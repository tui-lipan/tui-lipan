//! Markdown formatter for [`DocumentView`](super::DocumentView).
//!
//! This formatter supports common markdown structures used in TUI previews:
//! headings, fenced code blocks, tables, blockquotes, lists, horizontal rules,
//! and inline styles (emphasis/strong/strikethrough/code/link).

use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;
use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::style::{Span, Style};

use super::format::{
    ColumnAlign, ContentFormatter, DocumentStyles, FormatInput, FormattedBlock,
    FormattedBlockQuote, FormattedCodeBlock, FormattedDiagramBlock, FormattedDocument,
    FormattedLine, FormattedLink, FormattedList, FormattedListItem, FormattedTable,
};

/// Formatter that renders markdown into structured display blocks.
#[derive(Clone, Debug)]
pub struct MarkdownFormatter {
    /// Style set used for markdown elements.
    pub styles: DocumentStyles,
    /// Whether soft line breaks inside paragraphs are rendered as spaces.
    pub soft_break_as_space: bool,
    /// Whether blank source lines and Markdown block spacers are collapsed in output blocks.
    pub compact_blocks: bool,
    /// Whether fenced ```mermaid blocks are rendered as diagrams. When
    /// false, mermaid fences fall through to ordinary code-block rendering.
    pub render_diagrams: bool,
}

impl Default for MarkdownFormatter {
    fn default() -> Self {
        Self {
            styles: DocumentStyles::default(),
            soft_break_as_space: true,
            compact_blocks: false,
            render_diagrams: true,
        }
    }
}

impl MarkdownFormatter {
    /// Override element styles.
    pub fn styles(mut self, styles: DocumentStyles) -> Self {
        self.styles = styles;
        self
    }

    /// Treat soft line breaks as spaces when formatting inline markdown.
    pub fn soft_break_as_space(mut self, enabled: bool) -> Self {
        self.soft_break_as_space = enabled;
        self
    }

    /// Collapse blank source lines and Markdown block spacers for a denser preview.
    pub fn compact_blocks(mut self, enabled: bool) -> Self {
        self.compact_blocks = enabled;
        self
    }

    /// Render fenced ```mermaid blocks as diagrams (default: true). When
    /// disabled, mermaid fences render as plain code blocks.
    pub fn render_diagrams(mut self, enabled: bool) -> Self {
        self.render_diagrams = enabled;
        self
    }
}

impl ContentFormatter for MarkdownFormatter {
    fn clone_box(&self) -> Box<dyn ContentFormatter> {
        Box::new(self.clone())
    }

    fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
        let styles = if self.styles == DocumentStyles::default() {
            input.document_styles.unwrap_or(&self.styles)
        } else {
            &self.styles
        };
        let lines: Vec<(usize, String)> = if input.value.is_empty() {
            vec![(0, String::new())]
        } else {
            input
                .value
                .split('\n')
                .enumerate()
                .map(|(i, line)| (i, line.to_string()))
                .collect()
        };

        FormattedDocument {
            blocks: parse_blocks(&lines, self, styles),
        }
    }

    fn cache_key(&self) -> u64 {
        let mut h = FxHasher::default();
        self.soft_break_as_space.hash(&mut h);
        self.compact_blocks.hash(&mut h);
        self.render_diagrams.hash(&mut h);
        self.styles.heading_styles.hash(&mut h);
        self.styles.code_inline_style.hash(&mut h);
        self.styles.code_block_style.hash(&mut h);
        self.styles.emphasis_style.hash(&mut h);
        self.styles.strong_style.hash(&mut h);
        self.styles.strikethrough_style.hash(&mut h);
        self.styles.link_style.hash(&mut h);
        self.styles.blockquote_bar_style.hash(&mut h);
        self.styles.table_border_style.hash(&mut h);
        self.styles.table_header_style.hash(&mut h);
        self.styles.hr_style.hash(&mut h);
        h.finish()
    }

    fn measure_cache_key(&self) -> u64 {
        let mut h = FxHasher::default();
        self.soft_break_as_space.hash(&mut h);
        self.compact_blocks.hash(&mut h);
        self.render_diagrams.hash(&mut h);
        h.finish()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn parse_blocks(
    lines: &[(usize, String)],
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
) -> Vec<FormattedBlock> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let (source_line, raw) = (&lines[i].0, lines[i].1.as_str());
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            if !fmt.compact_blocks {
                // Preserve explicit blank lines from source for readable spacing.
                out.push(FormattedBlock::Lines(vec![FormattedLine {
                    spans: vec![Span::new("")],
                    source_line: *source_line,
                    indent: 0,
                    links: Vec::new(),
                }]));
            }
            i += 1;
            continue;
        }

        // Fenced code block: ```lang ... ```
        if let Some(lang) = parse_fence_start(trimmed) {
            let start_line = *source_line;
            i += 1;
            let mut code_lines = Vec::new();
            let mut fence_closed = false;
            while i < lines.len() {
                let t = lines[i].1.trim();
                if t.starts_with("```") {
                    i += 1;
                    fence_closed = true;
                    break;
                }
                code_lines.push(lines[i].1.clone());
                i += 1;
            }

            let block = build_fenced_block(lang, code_lines, fence_closed, start_line, fmt);
            push_fenced_block(&mut out, block, start_line, fmt);
            continue;
        }

        // Heading: #..######
        if let Some((level, content)) = parse_heading(trimmed) {
            let style = styles.heading_styles[level.saturating_sub(1).min(5)];
            let (spans, links) = parse_inline(content, fmt, styles, style);
            out.push(FormattedBlock::Lines(vec![FormattedLine {
                spans,
                source_line: *source_line,
                indent: 0,
                links,
            }]));
            i += 1;
            continue;
        }

        // Horizontal rule
        if is_horizontal_rule(trimmed) {
            out.push(FormattedBlock::HorizontalRule {
                source_line: *source_line,
            });
            i += 1;
            continue;
        }

        // Table: header row + separator row + data rows
        if i + 1 < lines.len() {
            let row0 = lines[i].1.as_str();
            let row1 = lines[i + 1].1.as_str();
            if looks_like_table_row(row0) && looks_like_table_separator(row1) {
                let start_line = *source_line;
                let headers = parse_table_cells(row0);
                let alignments = parse_table_alignments(row1);
                i += 2;

                let mut rows = Vec::new();
                while i < lines.len() {
                    let row = lines[i].1.as_str();
                    if row.trim().is_empty() || !looks_like_table_row(row) {
                        break;
                    }
                    rows.push(parse_table_cells(row));
                    i += 1;
                }

                let header_spans: Vec<Vec<Span>> = headers
                    .iter()
                    .map(|cell| parse_inline(cell, fmt, styles, styles.table_header_style).0)
                    .collect();
                let row_spans: Vec<Vec<Vec<Span>>> = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| parse_inline(cell, fmt, styles, Style::default()).0)
                            .collect()
                    })
                    .collect();

                out.push(FormattedBlock::Table(FormattedTable {
                    headers: header_spans,
                    rows: row_spans,
                    alignments,
                    source_line_start: start_line,
                }));
                continue;
            }
        }

        // Blockquote (supports nesting by computing minimum quote depth)
        if is_blockquote_line(trimmed) {
            let start_line = *source_line;
            let mut quote_lines: Vec<(usize, String)> = Vec::new();
            while i < lines.len() {
                let t = lines[i].1.trim_start();
                if !is_blockquote_line(t) {
                    break;
                }
                quote_lines.push((lines[i].0, lines[i].1.clone()));
                i += 1;
            }

            let depth = quote_lines
                .iter()
                .map(|(_, l)| quote_depth(l.trim_start()))
                .min()
                .unwrap_or(1)
                .max(1);

            let stripped: Vec<(usize, String)> = quote_lines
                .into_iter()
                .map(|(line_no, line)| (line_no, strip_quote_depth(line.as_str(), depth)))
                .collect();

            out.push(FormattedBlock::BlockQuote(FormattedBlockQuote {
                blocks: parse_blocks(&stripped, fmt, styles),
                depth,
                source_line_start: start_line,
            }));
            continue;
        }

        // Ordered / unordered list (indent-aware; supports nested lists)
        if parse_indented_list_line(lines[i].1.as_str()).is_some()
            && let Some(list) = parse_list_at(lines, &mut i, 0, fmt, styles)
        {
            out.push(FormattedBlock::List(list));
            continue;
        }

        // Paragraph: collect consecutive normal lines.
        let mut para_lines = Vec::new();
        while i < lines.len() {
            let (line_no, line) = (lines[i].0, lines[i].1.as_str());
            let t = line.trim();
            if t.is_empty()
                || parse_fence_start(t).is_some()
                || parse_heading(t).is_some()
                || is_horizontal_rule(t)
                || is_blockquote_line(t)
                || parse_indented_list_line(line).is_some()
            {
                break;
            }
            if i + 1 < lines.len()
                && looks_like_table_row(line)
                && looks_like_table_separator(lines[i + 1].1.as_str())
            {
                break;
            }

            let (spans, links) = parse_inline(line, fmt, styles, Style::default());
            para_lines.push(FormattedLine {
                spans,
                source_line: line_no,
                indent: 0,
                links,
            });
            i += 1;
        }

        if para_lines.is_empty() {
            // Safety net to avoid infinite loop on unexpected content.
            out.push(FormattedBlock::Lines(vec![FormattedLine {
                spans: vec![Span::new(raw)],
                source_line: *source_line,
                indent: 0,
                links: Vec::new(),
            }]));
            i += 1;
        } else {
            out.push(FormattedBlock::Lines(para_lines));
        }
    }

    out
}

fn push_fenced_block(
    out: &mut Vec<FormattedBlock>,
    block: FormattedBlock,
    source_line: usize,
    fmt: &MarkdownFormatter,
) {
    if !fmt.compact_blocks && previous_block_has_content(out.last()) {
        out.push(blank_line_block(source_line));
    }
    out.push(block);
}

fn build_fenced_block(
    lang: Option<&str>,
    code_lines: Vec<String>,
    fence_closed: bool,
    source_line_start: usize,
    fmt: &MarkdownFormatter,
) -> FormattedBlock {
    let code = Arc::<str>::from(code_lines.join("\n"));
    let is_mermaid = lang.is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"));
    // Only commit to rendering a diagram once the fence actually closes.
    // While the user is mid-typing the opening fence, the heavy diagram would
    // otherwise pop in and reflow the document.
    if is_mermaid
        && fence_closed
        && fmt.render_diagrams
        && let Ok(diagram) = super::mermaid::parse(&code)
    {
        return FormattedBlock::Diagram(FormattedDiagramBlock {
            diagram,
            source_code: code,
            source_line_start,
        });
    }

    FormattedBlock::CodeBlock(FormattedCodeBlock {
        language: lang.map(Arc::from),
        code,
        source_line_start,
    })
}

fn previous_block_has_content(block: Option<&FormattedBlock>) -> bool {
    match block {
        Some(FormattedBlock::Lines(lines)) => lines.iter().any(formatted_line_has_content),
        Some(_) => true,
        None => false,
    }
}

fn formatted_line_has_content(line: &FormattedLine) -> bool {
    line.spans
        .iter()
        .any(|span| !span.content.as_ref().trim().is_empty())
}

fn blank_line_block(source_line: usize) -> FormattedBlock {
    FormattedBlock::Lines(vec![FormattedLine {
        spans: vec![Span::new("")],
        source_line,
        indent: 0,
        links: Vec::new(),
    }])
}

fn parse_inline(
    text: &str,
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
    base_style: Style,
) -> (Vec<Span>, Vec<FormattedLink>) {
    let mut spans = Vec::new();
    let mut links = Vec::new();

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let inline_prefix_len = if needs_inline_prefix(text) { 1 } else { 0 };
    let inline_source = if inline_prefix_len == 0 {
        text.into()
    } else {
        format!("x{text}")
    };
    let parser = Parser::new_ext(&inline_source, options);

    let mut style_stack = vec![base_style];
    let mut byte_cursor = 0usize;
    let mut link_stack: Vec<(Arc<str>, usize)> = Vec::new();
    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Emphasis => style_stack.push(styles.emphasis_style),
                Tag::Strong => style_stack.push(styles.strong_style),
                Tag::Strikethrough => style_stack.push(styles.strikethrough_style),
                Tag::Link { dest_url, .. } => {
                    style_stack.push(styles.link_style);
                    link_stack.push((Arc::from(dest_url.as_ref()), byte_cursor));
                }
                _ => {}
            },
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough)
                if style_stack.len() > 1 =>
            {
                style_stack.pop();
            }
            Event::Text(t) if !t.is_empty() => {
                let len = t.len();
                spans.push(Span {
                    content: Arc::from(t.as_ref()),
                    style: merge_styles(&style_stack),
                    allow_row_style: true,
                });
                byte_cursor = byte_cursor.saturating_add(len);
            }
            Event::Code(code) => {
                let len = code.len();
                spans.push(Span {
                    content: Arc::from(code.as_ref()),
                    style: merge_styles(&style_stack).patch(styles.code_inline_style),
                    allow_row_style: true,
                });
                byte_cursor = byte_cursor.saturating_add(len);
            }
            Event::SoftBreak => {
                let text = if fmt.soft_break_as_space { " " } else { "\n" };
                spans.push(Span {
                    content: Arc::from(text),
                    style: merge_styles(&style_stack),
                    allow_row_style: true,
                });
                byte_cursor = byte_cursor.saturating_add(text.len());
            }
            Event::HardBreak => {
                spans.push(Span {
                    content: Arc::from("\n"),
                    style: merge_styles(&style_stack),
                    allow_row_style: true,
                });
                byte_cursor = byte_cursor.saturating_add(1);
            }
            Event::End(TagEnd::Link) => {
                if style_stack.len() > 1 {
                    style_stack.pop();
                }
                if let Some((url, start)) = link_stack.pop()
                    && byte_cursor > start
                {
                    links.push(FormattedLink {
                        start,
                        end: byte_cursor,
                        url,
                    });
                }
            }
            _ => {}
        }
    }

    strip_inline_prefix(&mut spans, &mut links, inline_prefix_len);

    if spans.is_empty() {
        spans.push(Span {
            content: Arc::from(text),
            style: base_style,
            allow_row_style: true,
        });
    }

    (spans, links)
}

fn strip_inline_prefix(spans: &mut Vec<Span>, links: &mut [FormattedLink], prefix_len: usize) {
    if prefix_len == 0 {
        return;
    }

    let mut remaining = prefix_len;
    while remaining > 0 && !spans.is_empty() {
        let span_len = spans[0].content.len();
        if span_len <= remaining {
            remaining -= span_len;
            spans.remove(0);
            continue;
        }

        let content = spans[0].content[remaining..].to_string();
        spans[0].content = Arc::from(content);
        remaining = 0;
    }

    for link in links.iter_mut() {
        link.start = link.start.saturating_sub(prefix_len);
        link.end = link.end.saturating_sub(prefix_len);
    }
}

fn needs_inline_prefix(text: &str) -> bool {
    parse_heading(text).is_some() || parse_list_marker(text).is_some() || is_horizontal_rule(text)
}

fn merge_styles(stack: &[Style]) -> Style {
    let mut out = Style::default();
    for s in stack {
        out = out.patch(*s);
    }
    out
}

fn parse_fence_start(trimmed: &str) -> Option<Option<&str>> {
    if !trimmed.starts_with("```") {
        return None;
    }
    let rest = trimmed.trim_start_matches('`').trim();
    if rest.is_empty() {
        Some(None)
    } else {
        let lang = rest.split_whitespace().next().unwrap_or_default();
        if lang.is_empty() {
            Some(None)
        } else {
            Some(Some(lang))
        }
    }
}

fn parse_heading(trimmed: &str) -> Option<(usize, &str)> {
    let mut level = 0usize;
    for ch in trimmed.chars() {
        if ch == '#' {
            level += 1;
        } else {
            break;
        }
    }
    if (1..=6).contains(&level)
        && trimmed
            .chars()
            .nth(level)
            .is_some_and(|c| c.is_whitespace())
    {
        Some((level, trimmed[level..].trim()))
    } else {
        None
    }
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let s = trimmed.replace(' ', "");
    if s.len() < 3 {
        return false;
    }
    let bytes = s.as_bytes();
    bytes.iter().all(|&b| b == b'-')
        || bytes.iter().all(|&b| b == b'*')
        || bytes.iter().all(|&b| b == b'_')
}

fn looks_like_table_row(line: &str) -> bool {
    let t = line.trim();
    t.contains('|') && parse_table_cells(t).len() >= 2
}

fn looks_like_table_separator(line: &str) -> bool {
    let cells = parse_table_cells(line.trim());
    if cells.len() < 2 {
        return false;
    }
    cells.iter().all(|c| {
        let t = c.trim();
        let core = t.trim_matches(':');
        core.len() >= 3 && core.chars().all(|ch| ch == '-')
    })
}

fn parse_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn parse_table_alignments(sep_line: &str) -> Vec<ColumnAlign> {
    parse_table_cells(sep_line)
        .into_iter()
        .map(|cell| {
            let t = cell.trim();
            let left = t.starts_with(':');
            let right = t.ends_with(':');
            match (left, right) {
                (true, true) => ColumnAlign::Center,
                (false, true) => ColumnAlign::Right,
                _ => ColumnAlign::Left,
            }
        })
        .collect()
}

fn is_blockquote_line(trimmed_start: &str) -> bool {
    trimmed_start.starts_with('>')
}

fn quote_depth(trimmed_start: &str) -> u16 {
    let mut depth = 0u16;
    let mut s = trimmed_start;
    loop {
        let t = s.trim_start();
        if !t.starts_with('>') {
            break;
        }
        depth += 1;
        s = t.trim_start_matches('>');
    }
    depth
}

fn strip_quote_depth(line: &str, depth: u16) -> String {
    let mut s = line.trim_start();
    for _ in 0..depth {
        if s.starts_with('>') {
            s = s.trim_start_matches('>').trim_start();
        }
    }
    s.to_string()
}

/// Visual column count for leading spaces and tabs (tab advances to next 4-col tab stop).
fn line_leading_columns(line: &str) -> (usize, &str) {
    let mut col = 0usize;
    let mut byte_i = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => {
                col += 1;
                byte_i += 1;
            }
            '\t' => {
                col += 4 - (col % 4);
                byte_i += ch.len_utf8();
            }
            _ => break,
        }
    }
    (col, &line[byte_i..])
}

fn strip_leading_columns(line: &str, columns: usize) -> String {
    let mut col = 0usize;
    let mut byte_i = 0usize;
    for ch in line.chars() {
        let advance = match ch {
            ' ' => 1,
            '\t' => 4 - (col % 4),
            _ => break,
        };
        if col.saturating_add(advance) > columns {
            let remaining_cols = col.saturating_add(advance).saturating_sub(columns);
            let mut stripped = " ".repeat(remaining_cols);
            stripped.push_str(&line[byte_i + ch.len_utf8()..]);
            return stripped;
        }
        col += advance;
        byte_i += ch.len_utf8();
        if col >= columns {
            break;
        }
    }
    line[byte_i..].to_string()
}

fn leading_columns_from(line: &str, start_col: usize) -> usize {
    let mut col = start_col;
    for ch in line.chars() {
        match ch {
            ' ' => col += 1,
            '\t' => col += 4 - (col % 4),
            _ => break,
        }
    }
    col.saturating_sub(start_col)
}

/// List marker at any leading indent: `(marker_column, ordered, start_number, item_text)`.
fn parse_indented_list_line(line: &str) -> Option<(usize, bool, usize, &str)> {
    let (indent, rest) = line_leading_columns(line);
    let (ordered, num, content) = parse_list_marker(rest)?;
    Some((indent, ordered, num, content))
}

/// Parse a list whose first line is at `lines[*i]` with marker column ≥ `min_marker_indent`.
/// Advances `*i` past all consumed lines; leaves `*i` on the first line not in this list.
fn parse_list_at(
    lines: &[(usize, String)],
    i: &mut usize,
    min_marker_indent: usize,
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
) -> Option<FormattedList> {
    let (marker_col, list_ordered, first_num, _) =
        parse_indented_list_line(lines.get(*i)?.1.as_str())?;
    if marker_col < min_marker_indent {
        return None;
    }
    let list_start = if list_ordered { first_num } else { 1 };
    let source_line_start = lines[*i].0;

    let mut items: Vec<FormattedListItem> = Vec::new();

    while *i < lines.len() {
        let (line_no, raw) = &lines[*i];
        if raw.trim().is_empty() {
            break;
        }

        let (ind, rest) = line_leading_columns(raw);
        if ind < marker_col {
            break;
        }

        let Some((ord, _, item_text)) = parse_list_marker(rest) else {
            break;
        };

        if ind != marker_col || ord != list_ordered {
            break;
        }

        *i += 1;
        let content_col = marker_col.saturating_add(rest.len().saturating_sub(item_text.len()));
        let content = parse_list_item_body(
            lines,
            i,
            ListItemIndent {
                marker_col,
                content_col,
            },
            fmt,
            styles,
            item_text,
            *line_no,
        );
        items.push(FormattedListItem {
            content,
            source_line: *line_no,
        });
    }

    if items.is_empty() {
        return None;
    }

    Some(FormattedList {
        ordered: list_ordered,
        start: list_start,
        items,
        source_line_start,
    })
}

/// Lines after the list marker line for one item: continuations and nested lists.
#[derive(Clone, Copy)]
struct ListItemIndent {
    marker_col: usize,
    content_col: usize,
}

fn parse_list_item_body(
    lines: &[(usize, String)],
    i: &mut usize,
    indent: ListItemIndent,
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
    first_text: &str,
    first_line_no: usize,
) -> Vec<FormattedBlock> {
    let mut blocks: Vec<FormattedBlock> = Vec::new();
    if let Some(lang) = parse_fence_start(first_text.trim_start()) {
        let fence_col = indent
            .content_col
            .saturating_add(leading_columns_from(first_text, indent.content_col));
        let code = consume_fenced_list_block(lines, i, fence_col, lang, first_line_no, fmt, false);
        blocks.push(code);
        return parse_list_item_tail(lines, i, indent.marker_col, fmt, styles, blocks, Vec::new());
    }

    let (spans, links) = parse_inline(first_text, fmt, styles, Style::default());
    let para: Vec<FormattedLine> = vec![FormattedLine {
        spans,
        source_line: first_line_no,
        indent: 0,
        links,
    }];

    parse_list_item_tail(lines, i, indent.marker_col, fmt, styles, blocks, para)
}

fn parse_list_item_tail(
    lines: &[(usize, String)],
    i: &mut usize,
    marker_col: usize,
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
    mut blocks: Vec<FormattedBlock>,
    mut para: Vec<FormattedLine>,
) -> Vec<FormattedBlock> {
    while *i < lines.len() {
        let (_, raw) = &lines[*i];
        if raw.trim().is_empty() {
            let blank_start = *i;
            let mut next = *i + 1;
            while next < lines.len() && lines[next].1.trim().is_empty() {
                next += 1;
            }
            let Some(next_raw) = lines.get(next).map(|(_, raw)| raw.as_str()) else {
                break;
            };
            let (next_ind, _) = line_leading_columns(next_raw);
            if next_ind <= marker_col {
                break;
            }
            if !para.is_empty() {
                blocks.push(FormattedBlock::Lines(std::mem::take(&mut para)));
            }
            if !fmt.compact_blocks {
                for (source_line, _) in lines.iter().take(next).skip(blank_start) {
                    blocks.push(FormattedBlock::Lines(vec![FormattedLine {
                        spans: vec![Span::new("")],
                        source_line: *source_line,
                        indent: 0,
                        links: Vec::new(),
                    }]));
                }
            }
            *i = next;
            continue;
        }

        let (ind, rest) = line_leading_columns(raw);

        if ind <= marker_col {
            break;
        }

        if let Some((_ord, _, text)) = parse_list_marker(rest) {
            // `ind > marker_col` is guaranteed here (same-indent siblings break above).
            if !para.is_empty() {
                blocks.push(FormattedBlock::Lines(std::mem::take(&mut para)));
            }
            if let Some(nested) = parse_list_at(lines, i, marker_col + 1, fmt, styles) {
                blocks.push(FormattedBlock::List(nested));
                continue;
            }
            let (spans, links) = parse_inline(text.trim_start(), fmt, styles, Style::default());
            para.push(FormattedLine {
                spans,
                source_line: lines[*i].0,
                indent: 0,
                links,
            });
            *i += 1;
            continue;
        }

        if let Some(lang) = parse_fence_start(rest.trim_start()) {
            if !para.is_empty() {
                blocks.push(FormattedBlock::Lines(std::mem::take(&mut para)));
            }
            let start_line = lines[*i].0;
            blocks.push(consume_fenced_list_block(
                lines, i, ind, lang, start_line, fmt, true,
            ));
            continue;
        }

        let (spans, links) = parse_inline(rest.trim_start(), fmt, styles, Style::default());
        para.push(FormattedLine {
            spans,
            source_line: lines[*i].0,
            indent: 0,
            links,
        });
        *i += 1;
    }

    if !para.is_empty() {
        blocks.push(FormattedBlock::Lines(para));
    }
    blocks
}

fn consume_fenced_list_block(
    lines: &[(usize, String)],
    i: &mut usize,
    fence_indent: usize,
    lang: Option<&str>,
    start_line: usize,
    fmt: &MarkdownFormatter,
    consume_opening_line: bool,
) -> FormattedBlock {
    if consume_opening_line {
        *i += 1;
    }
    let mut code_lines = Vec::new();
    let mut fence_closed = false;
    while *i < lines.len() {
        let raw = lines[*i].1.as_str();
        let stripped = strip_leading_columns(raw, fence_indent);
        if parse_fence_start(stripped.trim_start()).is_some() {
            *i += 1;
            fence_closed = true;
            break;
        }

        code_lines.push(stripped);
        *i += 1;
    }

    build_fenced_block(lang, code_lines, fence_closed, start_line, fmt)
}

fn parse_list_marker(trimmed: &str) -> Option<(bool, usize, &str)> {
    // Unordered: - item / * item / + item
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some((false, 1, rest));
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return Some((false, 1, rest));
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return Some((false, 1, rest));
    }

    // Ordered: 1. item or 1) item
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let (num, rest) = trimmed.split_at(digits);
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    let n = num.parse::<usize>().ok()?;
    Some((true, n, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn line_text(line: &FormattedLine) -> String {
        spans_text(&line.spans)
    }

    #[test]
    fn parse_heading_accepts_valid_headings() {
        assert_eq!(parse_heading("# Title"), Some((1, "Title")));
        assert_eq!(parse_heading("### Section"), Some((3, "Section")));
        assert_eq!(parse_heading("###### Deep"), Some((6, "Deep")));
    }

    #[test]
    fn parse_heading_rejects_invalid_headings() {
        assert_eq!(parse_heading("####### Too deep"), None);
        assert_eq!(parse_heading("#No space"), None);
        assert_eq!(parse_heading("plain text"), None);
    }

    #[test]
    fn horizontal_rule_detection_handles_true_and_false_cases() {
        assert!(is_horizontal_rule("---"));
        assert!(is_horizontal_rule("* * *"));
        assert!(is_horizontal_rule("_ _ _ _"));

        assert!(!is_horizontal_rule("--"));
        assert!(!is_horizontal_rule("-*-"));
        assert!(!is_horizontal_rule("---x"));
    }

    #[test]
    fn parse_table_row_and_cells_handles_edge_pipes() {
        let row = "| name | age | city |";
        assert!(looks_like_table_row(row));
        assert_eq!(parse_table_cells(row), vec!["name", "age", "city"],);

        assert_eq!(parse_table_cells("name|age"), vec!["name", "age"]);
        assert!(!looks_like_table_row("just one cell"));
    }

    #[test]
    fn parse_table_alignments_maps_markers() {
        let align = parse_table_alignments("| :--- | :---: | ---: | --- |");
        assert_eq!(
            align,
            vec![
                ColumnAlign::Left,
                ColumnAlign::Center,
                ColumnAlign::Right,
                ColumnAlign::Left,
            ],
        );
    }

    #[test]
    fn nested_unordered_list_parses_as_child_blocks() {
        let formatter = MarkdownFormatter::default();
        let input = "- outer one\n  - inner a\n  - inner b\n- outer two";
        let doc = formatter.format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let list = doc.blocks.iter().find_map(|block| match block {
            FormattedBlock::List(list) => Some(list),
            _ => None,
        });
        let list = list.expect("expected list");
        assert!(!list.ordered);
        assert_eq!(list.items.len(), 2);

        let outer0 = &list.items[0].content;
        assert!(
            outer0.iter().any(|b| matches!(b, FormattedBlock::List(_))),
            "first item should contain nested list"
        );
        let nested = outer0.iter().find_map(|b| match b {
            FormattedBlock::List(l) => Some(l),
            _ => None,
        });
        let nested = nested.expect("nested list");
        assert_eq!(nested.items.len(), 2);
        assert_eq!(
            line_text(match &nested.items[0].content[0] {
                FormattedBlock::Lines(ls) => &ls[0],
                _ => panic!("expected lines"),
            },),
            "inner a"
        );

        let outer1 = &list.items[1].content;
        let FormattedBlock::Lines(ls) = &outer1[0] else {
            panic!("expected lines");
        };
        assert_eq!(line_text(&ls[0]), "outer two");
    }

    #[test]
    fn list_item_indented_continuation_joins_first_item() {
        let formatter = MarkdownFormatter::default();
        let input = "- first line\n  more text\n- second";
        let doc = formatter.format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let list = doc
            .blocks
            .iter()
            .find_map(|block| match block {
                FormattedBlock::List(list) => Some(list),
                _ => None,
            })
            .expect("list");
        assert_eq!(list.items.len(), 2);
        let FormattedBlock::Lines(ls) = &list.items[0].content[0] else {
            panic!("expected lines");
        };
        assert_eq!(ls.len(), 2);
        assert_eq!(line_text(&ls[0]), "first line");
        assert_eq!(line_text(&ls[1]), "more text");
    }

    #[test]
    fn list_item_fenced_code_block_parses_as_code_block() {
        let formatter = MarkdownFormatter::default();
        let input = "2. Decide what to do:\n   ```text\n   M  src/widgets/scroll_view/reconcile/mod.rs\n   ?? samply-recording.json\n   ```\n   - keep intentional changes";
        let doc = formatter.format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let list = doc
            .blocks
            .iter()
            .find_map(|block| match block {
                FormattedBlock::List(list) => Some(list),
                _ => None,
            })
            .expect("list");
        assert!(list.ordered);
        assert_eq!(list.start, 2);

        let item = &list.items[0];
        assert!(matches!(item.content[0], FormattedBlock::Lines(_)));

        let code = item
            .content
            .iter()
            .find_map(|block| match block {
                FormattedBlock::CodeBlock(code) => Some(code),
                _ => None,
            })
            .expect("nested code block");
        assert_eq!(code.language.as_deref(), Some("text"));
        assert_eq!(
            code.code.as_ref(),
            "M  src/widgets/scroll_view/reconcile/mod.rs\n?? samply-recording.json"
        );

        assert!(
            item.content
                .iter()
                .any(|block| matches!(block, FormattedBlock::List(_))),
            "following nested bullet should remain a nested list"
        );
    }

    #[test]
    fn unordered_nested_fenced_code_block_after_blank_stays_in_item() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "- intro\n\n  ```rust\n  fn main() {}\n  ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(list) = &doc.blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(list.items.len(), 1);
        assert!(list.items[0]
            .content
            .iter()
            .any(|block| matches!(block, FormattedBlock::CodeBlock(code) if code.language.as_deref() == Some("rust") && code.code.as_ref() == "fn main() {}")));
    }

    #[test]
    fn ordered_blank_line_before_fence_stays_in_item() {
        let formatter = MarkdownFormatter::default().compact_blocks(true);
        let doc = formatter.format(FormatInput {
            value: "1. setup\n\n   ```text\n   cargo test\n   ```\n2. next",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(list) = &doc.blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(list.items.len(), 2);
        assert!(list.items[0]
            .content
            .iter()
            .any(|block| matches!(block, FormattedBlock::CodeBlock(code) if code.code.as_ref() == "cargo test")));
    }

    #[test]
    fn nested_bullet_after_fence_with_blank_separator_stays_nested() {
        let formatter = MarkdownFormatter::default().compact_blocks(true);
        let doc = formatter.format(FormatInput {
            value: "- outer\n  ```text\n  code\n  ```\n\n  - nested",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(list) = &doc.blocks[0] else {
            panic!("expected list");
        };
        assert!(
            list.items[0].content.iter().any(
                |block| matches!(block, FormattedBlock::List(nested) if nested.items.len() == 1)
            )
        );
    }

    #[test]
    fn marker_line_fence_parses_as_first_item_content() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "- ```rust\n  let x = 1;\n  ```\n1. ```text\n   ordered\n   ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(unordered) = &doc.blocks[0] else {
            panic!("expected unordered list");
        };
        let FormattedBlock::CodeBlock(unordered_code) = &unordered.items[0].content[0] else {
            panic!("expected unordered code block");
        };
        assert_eq!(unordered_code.language.as_deref(), Some("rust"));
        assert_eq!(unordered_code.code.as_ref(), "let x = 1;");

        let FormattedBlock::List(ordered) = &doc.blocks[1] else {
            panic!("expected ordered list");
        };
        let FormattedBlock::CodeBlock(ordered_code) = &ordered.items[0].content[0] else {
            panic!("expected ordered code block");
        };
        assert_eq!(ordered_code.language.as_deref(), Some("text"));
        assert_eq!(ordered_code.code.as_ref(), "ordered");
    }

    #[test]
    fn marker_line_fence_with_extra_spaces_strips_to_fence_column() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "-   ```rust\n    let x = 1;\n    ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(list) = &doc.blocks[0] else {
            panic!("expected list");
        };
        let FormattedBlock::CodeBlock(code) = &list.items[0].content[0] else {
            panic!("expected code block");
        };
        assert_eq!(code.language.as_deref(), Some("rust"));
        assert_eq!(code.code.as_ref(), "let x = 1;");
    }

    #[test]
    fn nested_fence_indent_stripping_preserves_partial_tab_remainder() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "1. item\n   ```text\n\tcode\n   ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(list) = &doc.blocks[0] else {
            panic!("expected list");
        };
        let code = list.items[0]
            .content
            .iter()
            .find_map(|block| match block {
                FormattedBlock::CodeBlock(code) => Some(code),
                _ => None,
            })
            .expect("code block");
        assert_eq!(code.code.as_ref(), " code");
    }

    #[test]
    fn parse_list_marker_supports_unordered_and_ordered_markers() {
        assert_eq!(parse_list_marker("- alpha"), Some((false, 1, "alpha")));
        assert_eq!(parse_list_marker("* beta"), Some((false, 1, "beta")));
        assert_eq!(parse_list_marker("+ gamma"), Some((false, 1, "gamma")));
        assert_eq!(parse_list_marker("12. delta"), Some((true, 12, "delta")));
        assert_eq!(parse_list_marker("1) first"), Some((true, 1, "first")));
        assert_eq!(parse_list_marker("2) second"), Some((true, 2, "second")));

        assert_eq!(parse_list_marker("1.delta"), None);
        assert_eq!(parse_list_marker("1)item"), None);
        assert_eq!(parse_list_marker("item"), None);
    }

    #[test]
    fn parse_inline_preserves_leading_list_markers() {
        let formatter = MarkdownFormatter::default();
        let styles = DocumentStyles::default();

        let (numbered_spans, _) = parse_inline("1. Mockup", &formatter, &styles, Style::default());
        assert_eq!(spans_text(&numbered_spans), "1. Mockup");

        let (dashed_spans, _) = parse_inline("- bullet", &formatter, &styles, Style::default());
        assert_eq!(spans_text(&dashed_spans), "- bullet");
    }

    #[test]
    fn mermaid_fence_formats_as_diagram_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nflowchart TD\nA[Start] --> B[End]\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::Diagram(_))
        ));
    }

    #[test]
    fn mermaid_fence_falls_back_to_code_block_when_diagrams_disabled() {
        let formatter = MarkdownFormatter::default().render_diagrams(false);
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nflowchart TD\nA[Start] --> B[End]\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::CodeBlock(_))
        ));
    }

    #[test]
    fn mermaid_flowchart_style_and_cylinder_still_format_as_diagram() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngraph TD\nA[Client Request] --> B{API Gateway}\nB --> I[(Database)]\nstyle A fill:#4CAF50,stroke:#333,color:#fff\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::Diagram(_))
        ));
    }

    #[test]
    fn mermaid_sequence_note_formats_as_diagram() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nsequenceDiagram\n    participant Queue\n    Note over Queue: Async processing\n    Queue->>Worker: consume(orderPlaced)\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::Diagram(_))
        ));
    }

    #[test]
    fn mermaid_gantt_fence_formats_as_diagram_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngantt\ntitle Release Plan\nsection Build\nDesign :done, design, 2026-01-01, 2d\nShip :milestone, after design, 0d\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::Diagram(_))
        ));
    }

    #[test]
    fn mermaid_common_grouping_blocks_format_as_diagrams() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: r#"```mermaid
graph TB
    subgraph Core Layers
        APP[AppRoot orchestration]
        SCR[screens ScreenReducer]
    end
    APP --> SCR
```

```mermaid
sequenceDiagram
    participant AppRoot
    participant Screen
    loop tick
        AppRoot->>Screen: update()
        Screen-->>AppRoot: Command::batch(...)
    end
```

```mermaid
stateDiagram-v2
    [*] --> Home
    Home --> Session : connected
    state Session {
        [*] --> Idle
        Idle --> Blocked : permission request
    }
    Session --> [*] : quit
```

```mermaid
flowchart LR
    subgraph Data & Transport
        client[client/]
        prompt[prompt/]
    end
    client --> prompt
```"#,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let diagram_blocks = doc
            .blocks
            .iter()
            .filter(|block| matches!(block, FormattedBlock::Diagram(_)))
            .count();
        assert_eq!(diagram_blocks, 4);
    }

    #[test]
    fn unclosed_mermaid_fence_falls_back_to_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngraph TD\nA --> B\n",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::CodeBlock(_))
        ));
    }

    #[test]
    fn invalid_mermaid_fence_falls_back_to_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nnotADiagram\nA --> B\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::CodeBlock(_))
        ));
    }

    #[test]
    fn invalid_mermaid_gantt_fence_falls_back_to_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngantt\ndateFormat MM/DD/YYYY\nsection Build\nDesign : 01/01/2026, 2d\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::CodeBlock(_))
        ));
    }

    #[test]
    fn non_mermaid_fence_stays_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```rust\nfn main() {}\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert!(matches!(
            doc.blocks.first(),
            Some(FormattedBlock::CodeBlock(_))
        ));
    }

    #[test]
    fn parse_inline_keeps_underscore_emphasis_recognized() {
        let formatter = MarkdownFormatter::default();
        let styles = DocumentStyles {
            emphasis_style: Style::new().italic(),
            ..DocumentStyles::default()
        };

        let (spans, _) = parse_inline("_asdf_", &formatter, &styles, Style::default());

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "asdf");
        assert_eq!(spans[0].style.italic, Some(true));
    }

    #[test]
    fn quote_depth_and_stripping_respect_requested_depth() {
        let line = "> > nested quote";
        assert_eq!(quote_depth(line), 2);
        assert_eq!(strip_quote_depth(line, 1), "> nested quote");
        assert_eq!(strip_quote_depth(line, 2), "nested quote");
    }

    #[test]
    fn markdown_formatter_format_sanity_check_with_mixed_content() {
        let input = "# Title\n\n1. first\n2. second\n\n| Name | Score |\n| :--- | ---: |\n| Ada | 99 |\n\n> quoted line\n\n---";
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let heading = match doc.blocks.first() {
            Some(FormattedBlock::Lines(lines)) => lines,
            _ => panic!("expected heading lines block at start"),
        };
        assert_eq!(line_text(&heading[0]), "Title");

        let list = doc.blocks.iter().find_map(|block| match block {
            FormattedBlock::List(list) => Some(list),
            _ => None,
        });
        let list = match list {
            Some(list) => list,
            None => panic!("expected list block"),
        };
        assert!(list.ordered);
        assert_eq!(list.start, 1);
        assert_eq!(list.items.len(), 2);

        let table = doc.blocks.iter().find_map(|block| match block {
            FormattedBlock::Table(table) => Some(table),
            _ => None,
        });
        let table = match table {
            Some(table) => table,
            None => panic!("expected table block"),
        };
        assert_eq!(
            table.alignments,
            vec![ColumnAlign::Left, ColumnAlign::Right]
        );
        assert_eq!(table.rows.len(), 1);
        assert_eq!(spans_text(&table.headers[0]), "Name");

        let quote = doc.blocks.iter().find_map(|block| match block {
            FormattedBlock::BlockQuote(quote) => Some(quote),
            _ => None,
        });
        let quote = match quote {
            Some(quote) => quote,
            None => panic!("expected blockquote block"),
        };
        assert_eq!(quote.depth, 1);

        let quote_text = quote.blocks.iter().find_map(|block| match block {
            FormattedBlock::Lines(lines) => lines.first().map(line_text),
            _ => None,
        });
        assert_eq!(quote_text.as_deref(), Some("quoted line"));

        assert!(
            doc.blocks
                .iter()
                .any(|block| matches!(block, FormattedBlock::HorizontalRule { .. }))
        );
    }

    #[test]
    fn markdown_headings_keep_numeric_and_dash_prefixes() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "## 1. Mockup\n## - Checklist",
            content_type: Some("markdown"),
            document_styles: None,
        });

        let headings: Vec<&FormattedLine> = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                FormattedBlock::Lines(lines) => lines.first(),
                _ => None,
            })
            .collect();

        assert_eq!(line_text(headings[0]), "1. Mockup");
        assert_eq!(line_text(headings[1]), "- Checklist");
    }

    #[test]
    fn default_markdown_formatter_uses_theme_document_styles() {
        let formatter = MarkdownFormatter::default();
        let themed = DocumentStyles {
            heading_styles: [Style::new().fg(crate::style::Color::Red); 6],
            link_style: Style::new().fg(crate::style::Color::Green).underline(),
            ..DocumentStyles::default()
        };

        let doc = formatter.format(FormatInput {
            value: "# Title\n\n[link](https://example.com)",
            content_type: Some("markdown"),
            document_styles: Some(&themed),
        });

        let heading = match doc.blocks.first() {
            Some(FormattedBlock::Lines(lines)) => lines,
            _ => panic!("expected heading lines block at start"),
        };
        assert_eq!(
            heading[0].spans[0].style.fg,
            Some(crate::style::Paint::Solid(crate::style::Color::Red))
        );

        let paragraph = match &doc.blocks[2] {
            FormattedBlock::Lines(lines) => lines,
            _ => panic!("expected paragraph block"),
        };
        assert_eq!(
            paragraph[0].spans[0].style.fg,
            Some(crate::style::Paint::Solid(crate::style::Color::Green))
        );
        assert_eq!(paragraph[0].spans[0].style.underline, Some(true));
    }
}
