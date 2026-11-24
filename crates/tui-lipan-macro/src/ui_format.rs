use proc_macro2::{LineColumn, TokenStream as TokenStream2};
use quote::quote;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Macro};

use crate::format_common::{
    FormatError, NestedMacroHooks, contains_comments, format_expr, format_pat, indent_block,
    line_starts, offset_for, skip_prefixed_string, skip_quoted_string,
};
use crate::ui_ast::{UiInput, UiItem};

const INDENT: &str = "    ";
const INLINE_WIDTH: usize = 88;
const EXPR_WRAPPER_PREFIX: &str = "const _: () = { ";
const INVALID_NESTED_UI_WRAPPER: &str = "invalid nested ui wrapper";

const UI_NESTED_HOOKS: NestedMacroHooks = NestedMacroHooks {
    contains_target_macro: expr_contains_ui_or_rsx_macro,
    format_wrapped_file: format_nested_wrapped_file,
    wrapper_prefix: EXPR_WRAPPER_PREFIX,
    invalid_wrapper_message: INVALID_NESTED_UI_WRAPPER,
};

#[derive(Clone)]
struct Rendered {
    text: String,
    multiline: bool,
}

impl Rendered {
    fn new(text: String) -> Self {
        let multiline = text.contains('\n');
        Self { text, multiline }
    }
}

struct LocatedMacro {
    path: syn::Path,
    tokens: TokenStream2,
    start: LineColumn,
    end: LineColumn,
    open_delimiter: char,
    close_delimiter: char,
}

#[derive(Default)]
struct UiMacroCollector {
    macros: Vec<LocatedMacro>,
}

impl<'ast> Visit<'ast> for UiMacroCollector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if is_ui_macro(mac) {
            let span = mac.span();
            self.macros.push(LocatedMacro {
                path: mac.path.clone(),
                tokens: mac.tokens.clone(),
                start: span.start(),
                end: mac.delimiter.span().close().end(),
                open_delimiter: open_delimiter(&mac.delimiter),
                close_delimiter: close_delimiter(&mac.delimiter),
            });
        }

        visit::visit_macro(self, mac);
    }
}

#[derive(Default)]
struct UiOrRsxExprDetector {
    found: bool,
}

impl<'ast> Visit<'ast> for UiOrRsxExprDetector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if is_ui_macro(mac) || is_rsx_macro(mac) {
            self.found = true;
            return;
        }

        visit::visit_macro(self, mac);
    }
}

fn is_ui_macro(mac: &Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "ui")
}

fn is_rsx_macro(mac: &Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "rsx")
}

fn close_delimiter(delimiter: &syn::MacroDelimiter) -> char {
    match delimiter {
        syn::MacroDelimiter::Paren(_) => ')',
        syn::MacroDelimiter::Brace(_) => '}',
        syn::MacroDelimiter::Bracket(_) => ']',
    }
}

fn open_delimiter(delimiter: &syn::MacroDelimiter) -> char {
    match delimiter {
        syn::MacroDelimiter::Paren(_) => '(',
        syn::MacroDelimiter::Brace(_) => '{',
        syn::MacroDelimiter::Bracket(_) => '[',
    }
}

fn expr_contains_ui_or_rsx_macro(expr: &Expr) -> bool {
    let mut detector = UiOrRsxExprDetector::default();
    detector.visit_expr(expr);
    detector.found
}

fn format_nested_wrapped_file(source: &str) -> Result<Option<String>, FormatError> {
    let mut output = source.to_string();
    let mut changed = false;

    if let Some(formatted) = crate::rsx_format::format_file_contents(&output)? {
        output = formatted;
        changed = true;
    }

    if let Some(formatted) = format_file_contents(&output)? {
        output = formatted;
        changed = true;
    }

    if changed { Ok(Some(output)) } else { Ok(None) }
}

pub(crate) fn format_file_contents(source: &str) -> Result<Option<String>, FormatError> {
    let file = syn::parse_file(source)?;
    let mut collector = UiMacroCollector::default();
    collector.visit_file(&file);

    if collector.macros.is_empty() {
        return Ok(None);
    }

    let line_starts = line_starts(source);
    let mut replacements = Vec::with_capacity(collector.macros.len());

    for mac in collector.macros {
        let start = offset_for(&line_starts, mac.start)?;
        let fallback_end = offset_for(&line_starts, mac.end)?;
        let end = macro_invocation_end(
            source,
            start,
            fallback_end,
            mac.open_delimiter,
            mac.close_delimiter,
        );

        if contains_comments(&source[start..end]) {
            continue;
        }

        let indent = line_indent(source, start);
        let replacement = format_macro_invocation(&mac.path, &mac.tokens, indent)?;
        replacements.push((start, end, replacement));
    }

    replacements.sort_by_key(|(start, _, _)| *start);

    for window in replacements.windows(2) {
        if let [current, next] = window
            && current.1 > next.0
        {
            return Err(FormatError::new("overlapping ui! macro spans"));
        }
    }

    let mut output = source.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }

    if output == source {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

fn macro_invocation_end(
    source: &str,
    start: usize,
    fallback_end: usize,
    open_delimiter: char,
    close_delimiter: char,
) -> usize {
    let Some(open_idx) = macro_open_delimiter(source, start, open_delimiter) else {
        return adjust_macro_end(source, fallback_end, close_delimiter);
    };

    matching_delimiter_end(source, open_idx, open_delimiter, close_delimiter)
        .unwrap_or_else(|| adjust_macro_end(source, fallback_end, close_delimiter))
}

fn macro_open_delimiter(source: &str, start: usize, open_delimiter: char) -> Option<usize> {
    let mut cursor = start;
    while cursor < source.len() {
        let ch = source[cursor..].chars().next()?;
        cursor += ch.len_utf8();

        if ch != '!' {
            continue;
        }

        while cursor < source.len() {
            let ch = source[cursor..].chars().next()?;
            if !ch.is_whitespace() {
                break;
            }
            cursor += ch.len_utf8();
        }

        return source[cursor..]
            .starts_with(open_delimiter)
            .then_some(cursor);
    }

    None
}

fn matching_delimiter_end(
    source: &str,
    open_idx: usize,
    open_delimiter: char,
    close_delimiter: char,
) -> Option<usize> {
    let bytes = source.as_bytes();
    let open = open_delimiter as u8;
    let close = close_delimiter as u8;
    let mut depth = 0usize;
    let mut cursor = open_idx;

    while cursor < bytes.len() {
        if let Some(next_cursor) = skip_scanner_literal_or_comment(bytes, cursor) {
            cursor = next_cursor;
            continue;
        }

        if bytes[cursor] == open {
            depth += 1;
        } else if bytes[cursor] == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(cursor + 1);
            }
        }

        cursor += 1;
    }

    None
}

fn skip_scanner_literal_or_comment(bytes: &[u8], cursor: usize) -> Option<usize> {
    if bytes[cursor] == b'/' && cursor + 1 < bytes.len() {
        if bytes[cursor + 1] == b'/' {
            return Some(skip_line_comment(bytes, cursor + 2));
        }

        if bytes[cursor + 1] == b'*' {
            return Some(skip_block_comment(bytes, cursor + 2));
        }
    }

    if bytes[cursor] == b'"' {
        return Some(skip_quoted_string(bytes, cursor + 1));
    }

    skip_prefixed_string(bytes, cursor)
}

fn skip_line_comment(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}

fn skip_block_comment(bytes: &[u8], mut cursor: usize) -> usize {
    let mut depth = 1usize;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
            depth += 1;
            cursor += 2;
            continue;
        }

        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return cursor;
            }
            continue;
        }

        cursor += 1;
    }

    bytes.len()
}

fn adjust_macro_end(source: &str, end: usize, close_delimiter: char) -> usize {
    if source[..end].trim_end().ends_with(close_delimiter) {
        return end;
    }

    let mut cursor = end;
    while cursor < source.len() {
        let Some(ch) = source[cursor..].chars().next() else {
            break;
        };

        if ch == close_delimiter {
            return cursor + ch.len_utf8();
        }

        if !ch.is_whitespace() {
            break;
        }

        cursor += ch.len_utf8();
    }

    end
}

fn format_macro_invocation(
    path: &syn::Path,
    tokens: &TokenStream2,
    line_indent: &str,
) -> Result<String, FormatError> {
    let input: UiInput = syn::parse2(tokens.clone())?;
    let path_text = quote!(#path).to_string();
    let root = format_item(&input.root)?;
    let root_is_parent = matches!(&input.root, UiItem::Parent { .. });

    if !root_is_parent && !root.multiline && path_text.len() + root.text.len() + 6 <= INLINE_WIDTH {
        return Ok(format!("{path_text}! {{ {} }}", root.text));
    }

    let mut output = String::new();
    output.push_str(&path_text);
    output.push_str("! {\n");
    output.push_str(&indent_block(&root.text, &format!("{line_indent}{INDENT}")));
    output.push('\n');
    output.push_str(line_indent);
    output.push('}');
    Ok(output)
}

fn format_item(item: &UiItem) -> Result<Rendered, FormatError> {
    match item {
        UiItem::Parent {
            expr,
            key,
            children,
        } => {
            let expr = format_expr(expr, Some(&UI_NESTED_HOOKS))?;
            let prefix = if let Some(key) = key {
                let key = format_expr(key, Some(&UI_NESTED_HOOKS))?;
                format!("{expr} @ {key}")
            } else {
                expr
            };

            if children.is_empty() {
                return Ok(Rendered::new(format!("{prefix} => {{ }}")));
            }

            let body = format_item_block(children)?.unwrap_or_default();
            Ok(Rendered::new(format!("{prefix} => {{\n{body}\n}}")))
        }
        UiItem::Leaf(expr, key) => {
            let expr = format_expr(expr, Some(&UI_NESTED_HOOKS))?;
            if let Some(key) = key {
                let key = format_expr(key, Some(&UI_NESTED_HOOKS))?;
                Ok(Rendered::new(format!("{expr} @ {key}")))
            } else {
                Ok(Rendered::new(expr))
            }
        }
        UiItem::For { pat, iter, body } => {
            let pat = format_pat(pat);
            let iter = format_expr(iter, Some(&UI_NESTED_HOOKS))?;

            if let Some(body) = format_item_block(body)? {
                Ok(Rendered::new(format!("for {pat} in {iter} {{\n{body}\n}}")))
            } else {
                Ok(Rendered::new(format!("for {pat} in {iter} {{}}")))
            }
        }
        UiItem::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond = format_expr(cond, Some(&UI_NESTED_HOOKS))?;
            let then_body = format_item_block(then_body)?;
            let mut output = match then_body {
                Some(body) => format!("if {cond} {{\n{body}\n}}"),
                None => format!("if {cond} {{}}"),
            };

            if let Some(else_body) = else_body {
                if let [inner_if @ UiItem::If { .. }] = else_body.as_slice() {
                    output.push_str(" else ");
                    output.push_str(&format_item(inner_if)?.text);
                } else if let Some(body) = format_item_block(else_body)? {
                    output.push_str(&format!(" else {{\n{body}\n}}"));
                } else {
                    output.push_str(" else {}");
                }
            }

            Ok(Rendered::new(output))
        }
    }
}

fn format_item_block(items: &[UiItem]) -> Result<Option<String>, FormatError> {
    if items.is_empty() {
        return Ok(None);
    }

    let lines = items
        .iter()
        .map(format_item)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|item| indent_block(&format!("{},", item.text), INDENT))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(lines))
}

fn line_indent(source: &str, offset: usize) -> &str {
    let line_start = source[..offset].rfind('\n').map_or(0, |idx| idx + 1);
    let bytes = source.as_bytes();
    let mut cursor = line_start;
    while cursor < offset && matches!(bytes[cursor], b' ' | b'\t') {
        cursor += 1;
    }
    &source[line_start..cursor]
}
