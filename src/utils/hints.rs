//! Generic terminal hint discovery.
//!
//! Built-in scanners are dependency-free. Regex-backed scanners are available
//! with the optional `hints-regex` feature.

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
    /// A relative, home-relative, or absolute path.
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

/// A hint found in a newline-delimited terminal snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintMatch {
    /// Zero-based line number.
    pub row: usize,
    /// Start display column, inclusive.
    pub start_col: usize,
    /// End display column, exclusive.
    pub end_col: usize,
    /// Matched text after trailing display punctuation is removed.
    pub text: String,
    /// The scanner that produced this match.
    pub kind: HintKind,
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
    pub fn scan(&self, text: &str) -> Vec<HintMatch> {
        let mut output = Vec::new();
        for (row, line) in text.split('\n').enumerate() {
            for (kind, scanner) in &self.scanners {
                let mut ranges = Vec::new();
                scanner.scan_line(line, &mut ranges);
                for range in ranges {
                    push_range(&mut output, row, line, range, *kind);
                }
            }
        }
        output.sort_by_key(|matched| (matched.row, matched.start_col, matched.end_col));
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

fn push_range(
    output: &mut Vec<HintMatch>,
    row: usize,
    line: &str,
    range: Range<usize>,
    kind: HintKind,
) {
    let start = range.start.min(line.len());
    let end = range.end.min(line.len());
    if start >= end || !line.is_char_boundary(start) || !line.is_char_boundary(end) {
        return;
    }
    let raw = &line[start..end];
    let trimmed = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
    if trimmed.is_empty() {
        return;
    }
    let end = start + trimmed.len();
    let start_col = display_column(line, start);
    let end_col = display_column(line, end);
    if start_col >= end_col
        || output.iter().any(|existing| {
            existing.row == row && start_col < existing.end_col && end_col > existing.start_col
        })
    {
        return;
    }
    output.push(HintMatch {
        row,
        start_col,
        end_col,
        text: trimmed.to_string(),
        kind,
    });
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
        let end = line[colon + 1..]
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace() || matches!(ch, '<' | '>'))
            .map(|(offset, _)| colon + 1 + offset)
            .unwrap_or(line.len());
        if end > colon + 1 {
            ranges.push(start..end);
        }
        cursor = end.max(colon + 1);
    }
    ranges
}

fn append_url_ranges(line: &str, out: &mut Vec<Range<usize>>) {
    out.extend(url_ranges(line));
}

fn path_ranges(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = line[cursor..].find('/') {
        let slash = cursor + relative;
        let start = if slash > 0 && line.as_bytes()[slash - 1] == b'/' {
            cursor = slash + 1;
            continue;
        } else if slash >= 2
            && line.as_bytes()[slash - 2] == b'.'
            && line.as_bytes()[slash - 1] == b'.'
        {
            slash - 2
        } else if slash >= 1 && matches!(line.as_bytes()[slash - 1], b'.' | b'~') {
            slash - 1
        } else {
            slash
        };
        if start > 0
            && !line[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"'))
        {
            cursor = slash + 1;
            continue;
        }
        let end = line[slash..]
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace() || *ch == ':')
            .map(|(offset, _)| slash + offset)
            .unwrap_or(line.len());
        let mut end = end;
        if end < line.len() && line.as_bytes()[end] == b':' {
            let port_start = end + 1;
            let port_end = line[port_start..]
                .char_indices()
                .find(|(_, ch)| !ch.is_ascii_digit())
                .map(|(offset, _)| port_start + offset)
                .unwrap_or(line.len());
            if port_end > port_start {
                end = port_end;
            }
        }
        if end > slash + 1 && end > start {
            ranges.push(start..end);
        }
        cursor = end.max(slash + 1);
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
            && line[start..end]
                .bytes()
                .any(|byte| byte.is_ascii_alphabetic())
        {
            ranges.push(start..end);
        }
        cursor = end.max(start + 1);
    }
    ranges
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
            .scan("你 mailto:user@example.test https://example.com/a). ./src/main.rs:12 deadbeef");
        assert_eq!(
            found
                .iter()
                .map(|matched| matched.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "mailto:user@example.test",
                "https://example.com/a",
                "./src/main.rs:12",
                "deadbeef"
            ]
        );
        assert_eq!(found[0].kind, HintKind::Url);
        assert_eq!(found[0].start_col, 3);
    }

    #[test]
    fn builder_and_closure_scanner_assign_custom_ids() {
        let found = HintScan::new()
            .custom(7u16, |line: &str, out: &mut Vec<Range<usize>>| {
                out.push(line.find("issue").unwrap()..line.len());
            })
            .scan("你 issue-123");
        assert_eq!(found[0].kind, HintKind::Custom(7));
        assert_eq!(found[0].start_col, 3);
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
        assert_eq!(found[0].start_col, 9);
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
