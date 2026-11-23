//! Markdown formatter for [`DocumentView`](super::DocumentView).
//!
//! This formatter supports common markdown structures used in TUI previews:
//! headings, fenced code blocks, tables, blockquotes, lists, horizontal rules,
//! and inline styles (emphasis/strong/strikethrough/code/link). Images become
//! links to their source, labelled by their alt text.

use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;
use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::style::{RowStylePolicy, Span, Style};

use super::format::{
    ColumnAlign, ContentFormatter, DocumentStyles, FormatInput, FormattedBlock,
    FormattedBlockQuote, FormattedCodeBlock, FormattedDiagramBlock, FormattedDocument,
    FormattedLine, FormattedLink, FormattedList, FormattedListItem, FormattedTable,
};

/// Vertical spacing policy between Markdown blocks in [`MarkdownFormatter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MarkdownSpacing {
    /// Balanced terminal typography (default).
    ///
    /// Separates sections and distinct paragraphs with a single blank row while
    /// tightening parent-to-child transitions that otherwise waste vertical
    /// space in a terminal:
    /// - headings stay attached to the block immediately following them, with a
    ///   blank row separating a heading from preceding content,
    /// - paragraphs ending with `:` stay attached to an immediately following
    ///   list, code block, diagram, table, or blockquote,
    /// - horizontal rules absorb adjacent blank rows because the rule itself
    ///   occupies a full divider row,
    /// - consecutive blank source lines collapse to at most one row, and
    ///   leading/trailing blank lines are trimmed.
    #[default]
    Balanced,
    /// Preserve explicit blank source lines between blocks.
    ///
    /// Runs of multiple blank source lines collapse to at most one row, leading
    /// and trailing blank lines are trimmed, and a visual spacer is inserted
    /// before a fenced code/diagram block that immediately follows non-blank
    /// content.
    Loose,
    /// Collapse all blank lines and block spacers so blocks sit flush against
    /// each other. Useful for dense previews such as chat cards or sidebars.
    Compact,
}

/// Formatter that renders markdown into structured display blocks.
#[derive(Clone, Debug)]
pub struct MarkdownFormatter {
    /// Style set used for markdown elements.
    pub styles: DocumentStyles,
    /// Whether soft line breaks inside paragraphs are rendered as spaces.
    pub soft_break_as_space: bool,
    /// Vertical spacing policy between rendered Markdown blocks.
    pub spacing: MarkdownSpacing,
    /// Whether fenced ```mermaid blocks are rendered as diagrams. When
    /// false, mermaid fences fall through to ordinary code-block rendering.
    pub render_diagrams: bool,
}

impl Default for MarkdownFormatter {
    fn default() -> Self {
        Self {
            styles: DocumentStyles::default(),
            soft_break_as_space: true,
            spacing: MarkdownSpacing::default(),
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

    /// Set the vertical spacing policy between rendered Markdown blocks.
    pub fn spacing(mut self, spacing: MarkdownSpacing) -> Self {
        self.spacing = spacing;
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
        self.spacing.hash(&mut h);
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
        self.spacing.hash(&mut h);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParsedBlockKind {
    Blank,
    Heading,
    Paragraph { ends_with_colon: bool },
    List,
    CodeBlock,
    Diagram,
    Table,
    BlockQuote,
    HorizontalRule,
}

struct ParsedBlock {
    kind: ParsedBlockKind,
    block: FormattedBlock,
    source_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FenceOpen<'a> {
    marker: char,
    len: usize,
    lang: Option<&'a str>,
    indent: usize,
}

fn parse_blocks(
    lines: &[(usize, String)],
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
) -> Vec<FormattedBlock> {
    let mut parsed: Vec<ParsedBlock> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let (source_line, raw) = (lines[i].0, lines[i].1.as_str());
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            parsed.push(ParsedBlock {
                kind: ParsedBlockKind::Blank,
                block: blank_line_block(source_line),
                source_line,
            });
            i += 1;
            continue;
        }

        // Fenced code block: ```lang ... ``` or ~~~lang ... ~~~
        if let Some(open) = parse_fence_start(raw) {
            parsed.push(consume_fenced_top_level_block(
                lines,
                &mut i,
                open,
                source_line,
                fmt,
            ));
            continue;
        }

        // Heading: #..######
        if let Some((level, content)) = parse_heading(trimmed) {
            let style = styles.heading_styles[level.saturating_sub(1).min(5)];
            let (spans, links) = parse_inline(content, fmt, styles, style);
            parsed.push(ParsedBlock {
                kind: ParsedBlockKind::Heading,
                block: FormattedBlock::Lines(vec![FormattedLine {
                    spans,
                    source_line,
                    indent: 0,
                    links,
                }]),
                source_line,
            });
            i += 1;
            continue;
        }

        // Horizontal rule
        if is_horizontal_rule(trimmed) {
            parsed.push(ParsedBlock {
                kind: ParsedBlockKind::HorizontalRule,
                block: FormattedBlock::HorizontalRule { source_line },
                source_line,
            });
            i += 1;
            continue;
        }

        // Table: header row + separator row + data rows
        if i + 1 < lines.len() {
            let row0 = lines[i].1.as_str();
            let row1 = lines[i + 1].1.as_str();
            if looks_like_table_row(row0) && looks_like_table_separator(row1) {
                let start_line = source_line;
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

                parsed.push(ParsedBlock {
                    kind: ParsedBlockKind::Table,
                    block: FormattedBlock::Table(FormattedTable {
                        headers: header_spans,
                        rows: row_spans,
                        alignments,
                        source_line_start: start_line,
                    }),
                    source_line: start_line,
                });
                continue;
            }
        }

        // Blockquote (supports nesting by computing minimum quote depth)
        if is_blockquote_line(trimmed) {
            let start_line = source_line;
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

            parsed.push(ParsedBlock {
                kind: ParsedBlockKind::BlockQuote,
                block: FormattedBlock::BlockQuote(FormattedBlockQuote {
                    blocks: parse_blocks(&stripped, fmt, styles),
                    depth,
                    source_line_start: start_line,
                }),
                source_line: start_line,
            });
            continue;
        }

        // Ordered / unordered list (indent-aware; supports nested lists)
        if parse_indented_list_line(lines[i].1.as_str()).is_some()
            && let Some(list) = parse_list_at(lines, &mut i, 0, fmt, styles)
        {
            let start_line = list.source_line_start;
            parsed.push(ParsedBlock {
                kind: ParsedBlockKind::List,
                block: FormattedBlock::List(list),
                source_line: start_line,
            });
            continue;
        }

        // Paragraph: collect consecutive normal lines.
        let para_start = source_line;
        let mut para_lines = Vec::new();
        while i < lines.len() {
            let (line_no, line) = (lines[i].0, lines[i].1.as_str());
            let t = line.trim();
            if t.is_empty()
                || parse_fence_start(line).is_some()
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
            para_lines.push(FormattedLine {
                spans: vec![Span::new(raw)],
                source_line,
                indent: 0,
                links: Vec::new(),
            });
            i += 1;
        }
        let ends_with_colon = lines_end_with_colon(&para_lines);
        parsed.push(ParsedBlock {
            kind: ParsedBlockKind::Paragraph { ends_with_colon },
            block: FormattedBlock::Lines(para_lines),
            source_line: para_start,
        });
    }

    apply_block_spacing(parsed, fmt.spacing)
}

fn consume_fenced_top_level_block(
    lines: &[(usize, String)],
    i: &mut usize,
    open: FenceOpen<'_>,
    start_line: usize,
    fmt: &MarkdownFormatter,
) -> ParsedBlock {
    *i += 1;
    let mut code_lines = Vec::new();
    let mut fence_closed = false;
    while *i < lines.len() {
        let raw = lines[*i].1.as_str();
        if is_fence_close(raw, open) {
            *i += 1;
            fence_closed = true;
            break;
        }
        let code_line = if open.indent > 0 {
            strip_leading_columns(raw, open.indent)
        } else {
            raw.to_string()
        };
        code_lines.push(code_line);
        *i += 1;
    }

    let block = build_fenced_block(open.lang, code_lines, fence_closed, start_line, fmt);
    let kind = fenced_block_kind(&block);
    ParsedBlock {
        kind,
        block,
        source_line: start_line,
    }
}

fn fenced_block_kind(block: &FormattedBlock) -> ParsedBlockKind {
    match block {
        FormattedBlock::Diagram(_) => ParsedBlockKind::Diagram,
        _ => ParsedBlockKind::CodeBlock,
    }
}

fn apply_block_spacing(parsed: Vec<ParsedBlock>, spacing: MarkdownSpacing) -> Vec<FormattedBlock> {
    let has_content = parsed
        .iter()
        .any(|item| item.kind != ParsedBlockKind::Blank);
    if !has_content {
        return match spacing {
            MarkdownSpacing::Compact => Vec::new(),
            MarkdownSpacing::Balanced | MarkdownSpacing::Loose => {
                parsed.into_iter().take(1).map(|item| item.block).collect()
            }
        };
    }

    let mut out = Vec::with_capacity(parsed.len());
    let mut prev_kind: Option<ParsedBlockKind> = None;
    let mut prev_end_source_line: Option<usize> = None;
    let mut pending_blank_line: Option<usize> = None;

    for item in parsed {
        if item.kind == ParsedBlockKind::Blank {
            if prev_kind.is_some() && pending_blank_line.is_none() {
                pending_blank_line = Some(item.source_line);
            }
            continue;
        }

        if let Some(prev) = prev_kind
            && needs_blank_between(prev, item.kind, pending_blank_line.is_some(), spacing)
        {
            let spacer_line = pending_blank_line
                .unwrap_or_else(|| prev_end_source_line.unwrap_or(item.source_line));
            out.push(blank_line_block(spacer_line));
        }
        pending_blank_line = None;
        prev_kind = Some(item.kind);
        prev_end_source_line = last_visual_source_line(&item.block).or(Some(item.source_line));
        out.push(item.block);
    }

    out
}

fn last_visual_source_line(block: &FormattedBlock) -> Option<usize> {
    match block {
        FormattedBlock::Lines(lines) => lines.last().map(|line| line.source_line),
        FormattedBlock::HorizontalRule { source_line } => Some(*source_line),
        FormattedBlock::CodeBlock(code) => {
            let line_count = code.code.split('\n').count();
            Some(
                code.source_line_start
                    .saturating_add(line_count.saturating_sub(1)),
            )
        }
        FormattedBlock::Diagram(diagram) => Some(diagram.source_line_start),
        FormattedBlock::Table(table) => Some(table.source_line_start),
        FormattedBlock::BlockQuote(quote) => quote
            .blocks
            .iter()
            .rev()
            .find_map(last_visual_source_line)
            .or(Some(quote.source_line_start)),
        FormattedBlock::List(list) => list
            .items
            .last()
            .and_then(|item| {
                item.content
                    .iter()
                    .rev()
                    .find_map(last_visual_source_line)
                    .or(Some(item.source_line))
            })
            .or(Some(list.source_line_start)),
    }
}

fn needs_blank_between(
    prev: ParsedBlockKind,
    next: ParsedBlockKind,
    had_source_blank: bool,
    spacing: MarkdownSpacing,
) -> bool {
    match spacing {
        MarkdownSpacing::Compact => false,
        MarkdownSpacing::Loose => {
            had_source_blank
                || matches!(next, ParsedBlockKind::CodeBlock | ParsedBlockKind::Diagram)
        }
        MarkdownSpacing::Balanced => {
            // Horizontal rules occupy a full row with centered stroke and act
            // as their own visual divider.
            if prev == ParsedBlockKind::HorizontalRule || next == ParsedBlockKind::HorizontalRule {
                return false;
            }
            // Headings stay attached to the content that immediately follows.
            if prev == ParsedBlockKind::Heading {
                return false;
            }
            // A paragraph ending with `:` introduces an immediately following
            // list, code block, diagram, table, or blockquote.
            if matches!(
                prev,
                ParsedBlockKind::Paragraph {
                    ends_with_colon: true
                }
            ) && matches!(
                next,
                ParsedBlockKind::List
                    | ParsedBlockKind::CodeBlock
                    | ParsedBlockKind::Diagram
                    | ParsedBlockKind::Table
                    | ParsedBlockKind::BlockQuote
            ) {
                return false;
            }
            // Start each new heading section with a separating row above it.
            if next == ParsedBlockKind::Heading {
                return true;
            }
            // Fenced blocks after non-lead-in prose keep a separating row even
            // when the source omitted a blank line.
            if matches!(next, ParsedBlockKind::CodeBlock | ParsedBlockKind::Diagram) {
                return true;
            }
            had_source_blank
        }
    }
}

fn lines_end_with_colon(lines: &[FormattedLine]) -> bool {
    lines
        .iter()
        .rev()
        .flat_map(|line| line.spans.iter().rev())
        .flat_map(|span| span.content.chars().rev())
        .find(|ch| !ch.is_whitespace())
        == Some(':')
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
                // An image cannot be drawn inline, so it reads as a link to its
                // source, labelled by its alt text.
                Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
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
                    row_style_policy: RowStylePolicy::Full,
                });
                byte_cursor = byte_cursor.saturating_add(len);
            }
            Event::Code(code) => {
                let len = code.len();
                spans.push(Span {
                    content: Arc::from(code.as_ref()),
                    style: merge_styles(&style_stack).patch(styles.code_inline_style),
                    row_style_policy: RowStylePolicy::Full,
                });
                byte_cursor = byte_cursor.saturating_add(len);
            }
            Event::SoftBreak => {
                let text = if fmt.soft_break_as_space { " " } else { "\n" };
                spans.push(Span {
                    content: Arc::from(text),
                    style: merge_styles(&style_stack),
                    row_style_policy: RowStylePolicy::Full,
                });
                byte_cursor = byte_cursor.saturating_add(text.len());
            }
            Event::HardBreak => {
                spans.push(Span {
                    content: Arc::from("\n"),
                    style: merge_styles(&style_stack),
                    row_style_policy: RowStylePolicy::Full,
                });
                byte_cursor = byte_cursor.saturating_add(1);
            }
            Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                if let Some((url, start)) = link_stack.last()
                    && end == TagEnd::Image
                    && byte_cursor == *start
                {
                    // `![](path)` has no alt text: show the source instead.
                    spans.push(Span {
                        content: Arc::clone(url),
                        style: merge_styles(&style_stack),
                        row_style_policy: RowStylePolicy::Full,
                    });
                    byte_cursor = byte_cursor.saturating_add(url.len());
                }
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
            row_style_policy: RowStylePolicy::Full,
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

fn parse_fence_start(line: &str) -> Option<FenceOpen<'_>> {
    parse_fence_start_at(line, 0)
}

fn parse_fence_start_at(line: &str, container_col: usize) -> Option<FenceOpen<'_>> {
    let (indent, rest) = line_leading_columns(line);
    let rel_indent = indent.saturating_sub(container_col);
    if rel_indent > 3 {
        return None;
    }
    parse_fence_start_trimmed(rest, rel_indent)
}

fn parse_fence_start_trimmed(trimmed_start: &str, indent: usize) -> Option<FenceOpen<'_>> {
    let marker = trimmed_start.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed_start.chars().take_while(|&c| c == marker).count();
    if len < 3 {
        return None;
    }
    let info = trimmed_start[len..].trim();
    // CommonMark §4.5: backtick fence info strings may not contain backticks.
    if marker == '`' && info.contains('`') {
        return None;
    }
    let lang = info.split_whitespace().next().filter(|s| !s.is_empty());
    Some(FenceOpen {
        marker,
        len,
        lang,
        indent,
    })
}

fn is_fence_close(line: &str, open: FenceOpen<'_>) -> bool {
    is_fence_close_at(line, 0, open)
}

fn is_fence_close_at(line: &str, container_col: usize, open: FenceOpen<'_>) -> bool {
    let (indent, rest) = line_leading_columns(line);
    if indent.saturating_sub(container_col) > 3 {
        return false;
    }
    is_fence_close_trimmed(rest, open)
}

fn is_fence_close_trimmed(trimmed_start: &str, open: FenceOpen<'_>) -> bool {
    let count = trimmed_start
        .chars()
        .take_while(|&c| c == open.marker)
        .count();
    count >= open.len && trimmed_start[count..].trim().is_empty()
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
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let marker_extra_indent = leading_columns_from(first_text, indent.content_col);
    if marker_extra_indent <= 3
        && let Some(open) = parse_fence_start_trimmed(first_text.trim_start(), marker_extra_indent)
    {
        let code = consume_fenced_list_block(
            lines,
            i,
            indent.content_col,
            open,
            first_line_no,
            fmt,
            false,
        );
        let kind = fenced_block_kind(&code);
        blocks.push(ParsedBlock {
            kind,
            block: code,
            source_line: first_line_no,
        });
        return parse_list_item_tail(lines, i, indent, fmt, styles, blocks, Vec::new());
    }

    let (spans, links) = parse_inline(first_text, fmt, styles, Style::default());
    let para: Vec<FormattedLine> = vec![FormattedLine {
        spans,
        source_line: first_line_no,
        indent: 0,
        links,
    }];

    parse_list_item_tail(lines, i, indent, fmt, styles, blocks, para)
}

fn push_list_para(blocks: &mut Vec<ParsedBlock>, para: &mut Vec<FormattedLine>) {
    if para.is_empty() {
        return;
    }
    let lines = std::mem::take(para);
    let source_line = lines[0].source_line;
    let ends_with_colon = lines_end_with_colon(&lines);
    blocks.push(ParsedBlock {
        kind: ParsedBlockKind::Paragraph { ends_with_colon },
        block: FormattedBlock::Lines(lines),
        source_line,
    });
}

fn parse_list_item_tail(
    lines: &[(usize, String)],
    i: &mut usize,
    indent: ListItemIndent,
    fmt: &MarkdownFormatter,
    styles: &DocumentStyles,
    mut blocks: Vec<ParsedBlock>,
    mut para: Vec<FormattedLine>,
) -> Vec<FormattedBlock> {
    let marker_col = indent.marker_col;
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
            push_list_para(&mut blocks, &mut para);
            for (source_line, _) in lines.iter().take(next).skip(blank_start) {
                blocks.push(ParsedBlock {
                    kind: ParsedBlockKind::Blank,
                    block: blank_line_block(*source_line),
                    source_line: *source_line,
                });
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
            push_list_para(&mut blocks, &mut para);
            if let Some(nested) = parse_list_at(lines, i, marker_col + 1, fmt, styles) {
                let start_line = nested.source_line_start;
                blocks.push(ParsedBlock {
                    kind: ParsedBlockKind::List,
                    block: FormattedBlock::List(nested),
                    source_line: start_line,
                });
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

        let container_col = indent.content_col.min(ind);
        if let Some(open) = parse_fence_start_at(raw, container_col) {
            push_list_para(&mut blocks, &mut para);
            let start_line = lines[*i].0;
            let code =
                consume_fenced_list_block(lines, i, container_col, open, start_line, fmt, true);
            let kind = fenced_block_kind(&code);
            blocks.push(ParsedBlock {
                kind,
                block: code,
                source_line: start_line,
            });
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

    push_list_para(&mut blocks, &mut para);
    apply_block_spacing(blocks, fmt.spacing)
}

fn consume_fenced_list_block(
    lines: &[(usize, String)],
    i: &mut usize,
    container_col: usize,
    open: FenceOpen<'_>,
    start_line: usize,
    fmt: &MarkdownFormatter,
    consume_opening_line: bool,
) -> FormattedBlock {
    if consume_opening_line {
        *i += 1;
    }
    let fence_col = container_col.saturating_add(open.indent);
    let mut code_lines = Vec::new();
    let mut fence_closed = false;
    while *i < lines.len() {
        let raw = lines[*i].1.as_str();
        if is_fence_close_at(raw, container_col, open) {
            *i += 1;
            fence_closed = true;
            break;
        }

        code_lines.push(strip_leading_columns(raw, fence_col));
        *i += 1;
    }

    build_fenced_block(open.lang, code_lines, fence_closed, start_line, fmt)
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
        std::assert_matches!(item.content[0], FormattedBlock::Lines(_));

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
        let formatter = MarkdownFormatter::default().spacing(MarkdownSpacing::Compact);
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
        let formatter = MarkdownFormatter::default().spacing(MarkdownSpacing::Compact);
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

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::Diagram(_)));
    }

    #[test]
    fn mermaid_fence_falls_back_to_code_block_when_diagrams_disabled() {
        let formatter = MarkdownFormatter::default().render_diagrams(false);
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nflowchart TD\nA[Start] --> B[End]\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::CodeBlock(_)));
    }

    #[test]
    fn mermaid_flowchart_style_and_cylinder_still_format_as_diagram() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngraph TD\nA[Client Request] --> B{API Gateway}\nB --> I[(Database)]\nstyle A fill:#4CAF50,stroke:#333,color:#fff\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::Diagram(_)));
    }

    #[test]
    fn mermaid_sequence_note_formats_as_diagram() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nsequenceDiagram\n    participant Queue\n    Note over Queue: Async processing\n    Queue->>Worker: consume(orderPlaced)\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::Diagram(_)));
    }

    #[test]
    fn mermaid_gantt_fence_formats_as_diagram_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngantt\ntitle Release Plan\nsection Build\nDesign :done, design, 2026-01-01, 2d\nShip :milestone, after design, 0d\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::Diagram(_)));
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

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::CodeBlock(_)));
    }

    #[test]
    fn invalid_mermaid_fence_falls_back_to_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\nnotADiagram\nA --> B\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::CodeBlock(_)));
    }

    #[test]
    fn invalid_mermaid_gantt_fence_falls_back_to_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```mermaid\ngantt\ndateFormat MM/DD/YYYY\nsection Build\nDesign : 01/01/2026, 2d\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::CodeBlock(_)));
    }

    #[test]
    fn non_mermaid_fence_stays_code_block() {
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "```rust\nfn main() {}\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });

        std::assert_matches!(doc.blocks.first(), Some(FormattedBlock::CodeBlock(_)));
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

        let paragraph = match &doc.blocks[1] {
            FormattedBlock::Lines(lines) => lines,
            _ => panic!("expected paragraph block immediately after heading"),
        };
        assert_eq!(
            paragraph[0].spans[0].style.fg,
            Some(crate::style::Paint::Solid(crate::style::Color::Green))
        );
        assert_eq!(paragraph[0].spans[0].style.underline, Some(true));
    }

    fn paragraph_links(value: &str) -> (String, Vec<FormattedLink>, Vec<Style>) {
        let styles = DocumentStyles {
            link_style: Style::new().fg(crate::style::Color::Green).underline(),
            ..DocumentStyles::default()
        };
        let doc = MarkdownFormatter::default().format(FormatInput {
            value,
            content_type: Some("markdown"),
            document_styles: Some(&styles),
        });
        let lines = match &doc.blocks[0] {
            FormattedBlock::Lines(lines) => lines,
            _ => panic!("expected paragraph block"),
        };
        (
            line_text(&lines[0]),
            lines[0].links.clone(),
            lines[0].spans.iter().map(|span| span.style).collect(),
        )
    }

    #[test]
    fn images_read_as_links_to_their_source() {
        let (text, links, styles) = paragraph_links("See ![Before and after](/tmp/shot.png) here");

        assert_eq!(text, "See Before and after here");
        assert_eq!(links.len(), 1);
        assert_eq!(&*links[0].url, "/tmp/shot.png");
        assert_eq!(&text[links[0].start..links[0].end], "Before and after");
        assert_eq!(styles[1].underline, Some(true));
        assert_eq!(styles[2].underline, None);
    }

    #[test]
    fn an_image_without_alt_text_shows_its_source() {
        let (text, links, styles) = paragraph_links("![](https://example.com/a.png)");

        assert_eq!(text, "https://example.com/a.png");
        assert_eq!(&*links[0].url, "https://example.com/a.png");
        assert_eq!((links[0].start, links[0].end), (0, text.len()));
        assert_eq!(styles[0].underline, Some(true));
    }

    #[test]
    fn four_backtick_fence_preserves_nested_three_backtick_fence() {
        let input = "\
A fence that contains another fence:

````markdown
Use this in a doc:

```rust
let label = \"ok\";
```
````

## Table";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        let code_blocks: Vec<&FormattedCodeBlock> = doc
            .blocks
            .iter()
            .filter_map(|block| match block {
                FormattedBlock::CodeBlock(code) => Some(code),
                _ => None,
            })
            .collect();
        assert_eq!(code_blocks.len(), 1);
        assert_eq!(code_blocks[0].language.as_deref(), Some("markdown"));
        assert_eq!(
            code_blocks[0].code.as_ref(),
            "Use this in a doc:\n\n```rust\nlet label = \"ok\";\n```"
        );
    }

    #[test]
    fn tilde_fence_preserves_inner_backticks_and_shorter_tildes() {
        let input = "~~~~markdown\n```rust\nfn main() {}\n```\n~~~\n~~~~";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert_eq!(doc.blocks.len(), 1);
        let FormattedBlock::CodeBlock(code) = &doc.blocks[0] else {
            panic!("expected code block");
        };
        assert_eq!(code.language.as_deref(), Some("markdown"));
        assert_eq!(code.code.as_ref(), "```rust\nfn main() {}\n```\n~~~");
    }

    #[test]
    fn closing_fence_with_info_string_does_not_close_outer_fence() {
        let input = "````markdown\n````rust\nlet x = 1;\n````";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert_eq!(doc.blocks.len(), 1);
        let FormattedBlock::CodeBlock(code) = &doc.blocks[0] else {
            panic!("expected code block");
        };
        assert_eq!(code.code.as_ref(), "````rust\nlet x = 1;");
    }

    #[test]
    fn backtick_inline_span_on_single_line_is_not_a_fenced_code_block() {
        let input = "```inline code```";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
            content_type: Some("markdown"),
            document_styles: None,
        });

        assert_eq!(doc.blocks.len(), 1);
        let FormattedBlock::Lines(lines) = &doc.blocks[0] else {
            panic!("expected paragraph, not code block");
        };
        assert_eq!(line_text(&lines[0]), "inline code");
    }

    #[test]
    fn list_item_preserves_nested_fence_inside_four_backtick_fence() {
        let input = "\
- Nested fence example:
  ````markdown
  ```rust
  let x = 1;
  ```
  ````";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
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
            .expect("nested code block");
        assert_eq!(code.language.as_deref(), Some("markdown"));
        assert_eq!(code.code.as_ref(), "```rust\nlet x = 1;\n```");
    }

    fn block_tags(doc: &FormattedDocument) -> Vec<&'static str> {
        doc.blocks
            .iter()
            .map(|block| match block {
                FormattedBlock::Lines(lines)
                    if lines.len() == 1
                        && lines[0]
                            .spans
                            .iter()
                            .all(|span| span.content.trim().is_empty()) =>
                {
                    "blank"
                }
                FormattedBlock::Lines(_) => "lines",
                FormattedBlock::List(_) => "list",
                FormattedBlock::CodeBlock(_) => "code",
                FormattedBlock::Diagram(_) => "diagram",
                FormattedBlock::Table(_) => "table",
                FormattedBlock::BlockQuote(_) => "quote",
                FormattedBlock::HorizontalRule { .. } => "hr",
            })
            .collect()
    }

    #[test]
    fn balanced_spacing_tightens_headings_lead_ins_and_rules() {
        let input = "\n\nIntro.\n\n\n## Lists\n\nBullets:\n\n- a\n- b\n\n## Code\n\nRust:\n\n```rust\nfn main() {}\n```\n\n> Quoted\n\n---\n\nTail.\n\n";
        let doc = MarkdownFormatter::default()
            .spacing(MarkdownSpacing::Balanced)
            .format(FormatInput {
                value: input,
                content_type: Some("markdown"),
                document_styles: None,
            });

        assert_eq!(
            block_tags(&doc),
            vec![
                "lines", // Intro.
                "blank", // section break before ## Lists
                "lines", // ## Lists
                "lines", // Bullets: (attached to heading)
                "list",  // - a, - b (attached to colon lead-in)
                "blank", // section break before ## Code
                "lines", // ## Code
                "lines", // Rust: (attached to heading)
                "code",  // rust block (attached to colon lead-in)
                "blank", // explicit blank before quote
                "quote", // > Quoted
                "hr",    // --- (absorbs surrounding blanks)
                "lines", // Tail.
            ]
        );
    }

    #[test]
    fn loose_and_compact_spacing_policies_differ_from_balanced() {
        let input = "\n\n## Title\n\n\nBody.\n\nNext.\n\n";

        let balanced = MarkdownFormatter::default()
            .spacing(MarkdownSpacing::Balanced)
            .format(FormatInput {
                value: input,
                content_type: Some("markdown"),
                document_styles: None,
            });
        assert_eq!(
            block_tags(&balanced),
            vec!["lines", "lines", "blank", "lines"]
        );

        let loose = MarkdownFormatter::default()
            .spacing(MarkdownSpacing::Loose)
            .format(FormatInput {
                value: input,
                content_type: Some("markdown"),
                document_styles: None,
            });
        assert_eq!(
            block_tags(&loose),
            vec!["lines", "blank", "lines", "blank", "lines"]
        );

        let compact = MarkdownFormatter::default()
            .spacing(MarkdownSpacing::Compact)
            .format(FormatInput {
                value: input,
                content_type: Some("markdown"),
                document_styles: None,
            });
        assert_eq!(block_tags(&compact), vec!["lines", "lines", "lines"]);
    }

    #[test]
    fn markdown_spacing_affects_formatter_cache_keys() {
        let balanced = MarkdownFormatter::default().spacing(MarkdownSpacing::Balanced);
        let loose = MarkdownFormatter::default().spacing(MarkdownSpacing::Loose);
        let compact = MarkdownFormatter::default().spacing(MarkdownSpacing::Compact);

        assert_ne!(balanced.cache_key(), loose.cache_key());
        assert_ne!(balanced.cache_key(), compact.cache_key());
        assert_ne!(loose.cache_key(), compact.cache_key());
        assert_ne!(balanced.measure_cache_key(), loose.measure_cache_key());
        assert_ne!(balanced.measure_cache_key(), compact.measure_cache_key());
        assert_ne!(loose.measure_cache_key(), compact.measure_cache_key());
    }

    #[test]
    fn document_view_markdown_spacing_preserves_existing_formatter_options() {
        let view = super::super::DocumentView::new("```mermaid\ngraph TD\nA --> B\n```")
            .markdown()
            .render_diagrams(false)
            .markdown_spacing(MarkdownSpacing::Compact);

        let formatter = view.formatter.as_deref().expect("formatter");
        let md = formatter
            .as_any()
            .downcast_ref::<MarkdownFormatter>()
            .expect("markdown formatter");
        assert!(!md.render_diagrams);
        assert_eq!(md.spacing, MarkdownSpacing::Compact);
    }

    #[test]
    fn list_item_rejects_fence_indented_four_columns_past_content() {
        // `- item` has content column 2, so 6 leading spaces (4 extra) is not a fence.
        let tail_input = "- item\n      ```text\n      body\n      ```";
        let tail_doc = MarkdownFormatter::default().format(FormatInput {
            value: tail_input,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(tail_list) = &tail_doc.blocks[0] else {
            panic!("expected list");
        };
        assert!(
            !tail_list.items[0]
                .content
                .iter()
                .any(|block| matches!(block, FormattedBlock::CodeBlock(_))),
            "4+ columns past list content column must not open a fenced code block"
        );

        // Marker-line fence with 4 extra spaces past content column 2 is also rejected.
        let marker_input = "-     ```text\n      body\n      ```";
        let marker_doc = MarkdownFormatter::default().format(FormatInput {
            value: marker_input,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::List(marker_list) = &marker_doc.blocks[0] else {
            panic!("expected list");
        };
        assert!(
            !marker_list.items[0]
                .content
                .iter()
                .any(|block| matches!(block, FormattedBlock::CodeBlock(_))),
            "marker-line fence with 4+ extra columns must not open a fenced code block"
        );
    }

    #[test]
    fn list_item_accepts_fence_indented_up_to_three_columns_past_content() {
        // `- item` has content column 2; 5 leading spaces is 3 extra columns (allowed).
        let input = "- item\n     ```rust\n     let x = 1;\n     ```";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
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
            .expect("fence 3 columns past content column should parse as code block");
        assert_eq!(code.language.as_deref(), Some("rust"));
        assert_eq!(code.code.as_ref(), "let x = 1;");
    }

    #[test]
    fn list_item_deeply_indented_backticks_do_not_close_fence() {
        // Content column is 2; 6 leading spaces (4 extra) inside the body must not close.
        let input = "- item\n  ```markdown\n      ```\n  still inside\n  ```";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: input,
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
        assert_eq!(code.language.as_deref(), Some("markdown"));
        assert_eq!(code.code.as_ref(), "    ```\nstill inside");
    }

    #[test]
    fn synthetic_balanced_spacer_inherits_previous_block_source_line() {
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: "paragraph\n## Heading",
            content_type: Some("markdown"),
            document_styles: None,
        });
        assert_eq!(block_tags(&doc), vec!["lines", "blank", "lines"]);

        let FormattedBlock::Lines(para) = &doc.blocks[0] else {
            panic!("expected paragraph");
        };
        let FormattedBlock::Lines(spacer) = &doc.blocks[1] else {
            panic!("expected synthetic spacer");
        };
        let FormattedBlock::Lines(heading) = &doc.blocks[2] else {
            panic!("expected heading");
        };
        assert_eq!(para[0].source_line, 0);
        assert_eq!(
            spacer[0].source_line, 0,
            "synthetic spacer must inherit preceding block source line, not steal heading's line"
        );
        assert_eq!(heading[0].source_line, 1);

        // Multi-line paragraph before heading inherits the last paragraph line.
        let multi = MarkdownFormatter::default().format(FormatInput {
            value: "line zero\nline one\n## Heading",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let FormattedBlock::Lines(multi_spacer) = &multi.blocks[1] else {
            panic!("expected synthetic spacer");
        };
        let FormattedBlock::Lines(multi_heading) = &multi.blocks[2] else {
            panic!("expected heading");
        };
        assert_eq!(multi_spacer[0].source_line, 1);
        assert_eq!(multi_heading[0].source_line, 2);
    }
}
