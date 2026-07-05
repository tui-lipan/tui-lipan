use super::MermaidParseError;

pub(crate) fn significant_lines(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let cleaned = clean_line(line);
            (!cleaned.is_empty()).then_some((idx + 1, cleaned))
        })
        .collect()
}

pub(crate) fn clean_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.starts_with("%%") {
        return String::new();
    }
    trimmed.to_string()
}

pub(crate) fn strip_quotes(value: &str) -> &str {
    value
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| value.trim())
}

pub(crate) fn parse_labelled_pair<'a>(
    line: &'a str,
    sep: &str,
    line_no: usize,
) -> Result<(&'a str, &'a str), MermaidParseError> {
    line.split_once(sep)
        .map(|(left, right)| (left.trim(), right.trim()))
        .filter(|(left, right)| !left.is_empty() && !right.is_empty())
        .ok_or_else(|| MermaidParseError::new(format!("expected `{sep}`"), Some(line_no)))
}
