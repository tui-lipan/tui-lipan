use proc_macro2::LineColumn;
use quote::quote;

const INDENT: &str = "    ";
const INLINE_WIDTH: usize = 88;

#[derive(Debug)]
pub(crate) struct FormatError {
    message: String,
}

impl FormatError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FormatError {}

impl From<syn::Error> for FormatError {
    fn from(error: syn::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub(crate) struct NestedMacroHooks {
    pub(crate) contains_target_macro: fn(&syn::Expr) -> bool,
    pub(crate) format_wrapped_file: fn(&str) -> Result<Option<String>, FormatError>,
    pub(crate) wrapper_prefix: &'static str,
    pub(crate) invalid_wrapper_message: &'static str,
}

pub(crate) fn indent_block(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| {
            let mut output = String::with_capacity(prefix.len() + line.len());
            output.push_str(prefix);
            output.push_str(line);
            output
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

pub(crate) fn offset_for(line_starts: &[usize], pos: LineColumn) -> Result<usize, FormatError> {
    let Some(line_start) = line_starts.get(pos.line.saturating_sub(1)) else {
        return Err(FormatError::new("invalid macro span line"));
    };
    Ok(line_start + pos.column)
}

pub(crate) fn format_expr(
    expr: &syn::Expr,
    nested_macro_hooks: Option<&NestedMacroHooks>,
) -> Result<String, FormatError> {
    if let Some(formatted) = try_format_vec_macro(expr, nested_macro_hooks)? {
        return Ok(formatted);
    }

    let file: syn::File = syn::parse_quote! {
        fn __rsx_fmt() {
            #expr;
        }
    };
    let rendered = prettyplease::unparse(&file);
    let mut lines = rendered.lines().collect::<Vec<_>>();
    lines.remove(0);
    lines.pop();

    let mut body = lines
        .into_iter()
        .map(|line| line.strip_prefix(INDENT).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");

    if body.ends_with(';') {
        body.pop();
    }

    body = format_vec_macros_in_text(&body, nested_macro_hooks)?;

    let Some(hooks) = nested_macro_hooks else {
        return Ok(body);
    };

    if !(hooks.contains_target_macro)(expr) {
        return Ok(body);
    }

    let wrapped = format!("{}{} }};\n", hooks.wrapper_prefix, body);
    match (hooks.format_wrapped_file)(&wrapped)? {
        Some(formatted) => extract_wrapped_expr(
            &formatted,
            hooks.wrapper_prefix,
            hooks.invalid_wrapper_message,
        ),
        None => Ok(body),
    }
}

pub(crate) fn format_pat(pat: &syn::Pat) -> String {
    let file: syn::File = syn::parse_quote! {
        fn __rsx_fmt_pat() {
            let #pat = ();
        }
    };
    let rendered = prettyplease::unparse(&file);
    let mut lines = rendered.lines();
    let _ = lines.next();
    let line = lines.next().unwrap_or_default();
    let line = line.strip_prefix(INDENT).unwrap_or(line);
    line.strip_prefix("let ")
        .and_then(|rest| rest.strip_suffix(" = ();"))
        .unwrap_or(line)
        .to_string()
}

pub(crate) fn contains_comments(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut idx = 0;

    while idx < bytes.len() {
        if let Some(next_idx) = skip_prefixed_string(bytes, idx) {
            idx = next_idx;
            continue;
        }

        match bytes[idx] {
            b'/' if idx + 1 < bytes.len() && matches!(bytes[idx + 1], b'/' | b'*') => {
                return true;
            }
            b'"' => {
                idx = skip_quoted_string(bytes, idx + 1);
            }
            _ => {
                idx += 1;
            }
        }
    }

    false
}

fn try_format_vec_macro(
    expr: &syn::Expr,
    nested_macro_hooks: Option<&NestedMacroHooks>,
) -> Result<Option<String>, FormatError> {
    let syn::Expr::Macro(m) = expr else {
        return Ok(None);
    };
    if !m.mac.path.is_ident("vec") {
        return Ok(None);
    }
    let stream = m.mac.tokens.clone();
    let inner: syn::Expr = match syn::parse2(quote! { [#stream] }) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let syn::Expr::Array(arr) = inner else {
        return Ok(None);
    };
    if arr.elems.is_empty() {
        return Ok(Some("vec![]".to_string()));
    }
    Ok(Some(format_vec_array(&arr, nested_macro_hooks)?))
}

fn format_vec_array(
    arr: &syn::ExprArray,
    nested_macro_hooks: Option<&NestedMacroHooks>,
) -> Result<String, FormatError> {
    let mut parts = Vec::with_capacity(arr.elems.len());
    for elem in &arr.elems {
        parts.push(format_expr(elem, nested_macro_hooks)?);
    }
    let single_line = format!("vec![{}]", parts.join(", "));
    let any_multiline = parts.iter().any(|s| s.contains('\n'));
    if single_line.len() <= INLINE_WIDTH && !any_multiline {
        return Ok(single_line);
    }
    let body = parts
        .iter()
        .map(|p| indent_block(p.trim_end(), INDENT))
        .collect::<Vec<_>>()
        .join(",\n");
    Ok(format!("vec![\n{body},\n]"))
}

fn format_vec_macros_in_text(
    text: &str,
    nested_macro_hooks: Option<&NestedMacroHooks>,
) -> Result<String, FormatError> {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < text.len() {
        let Some(found) = find_vec_macro(text, cursor) else {
            output.push_str(&text[cursor..]);
            break;
        };

        let Some(end) = matching_bracket_end(text.as_bytes(), found.open_bracket) else {
            output.push_str(&text[cursor..=found.name_start]);
            cursor = found.name_start + 1;
            continue;
        };

        let inner = &text[found.open_bracket + 1..end - 1];
        let expr = syn::parse_str::<syn::Expr>(&format!("[{inner}]"));
        let Ok(syn::Expr::Array(arr)) = expr else {
            output.push_str(&text[cursor..=found.name_start]);
            cursor = found.name_start + 1;
            continue;
        };

        output.push_str(&text[cursor..found.name_start]);
        let formatted = format_vec_array(&arr, nested_macro_hooks)?;
        output.push_str(&indent_subsequent_lines(
            &formatted,
            &line_prefix(text, found.name_start),
        ));
        cursor = end;
    }

    Ok(output)
}

struct VecMacroLocation {
    name_start: usize,
    open_bracket: usize,
}

fn find_vec_macro(text: &str, start: usize) -> Option<VecMacroLocation> {
    let bytes = text.as_bytes();
    let mut cursor = start;

    while cursor < bytes.len() {
        if let Some(next_cursor) = skip_literal_or_comment(bytes, cursor) {
            cursor = next_cursor;
            continue;
        }

        if is_vec_ident_at(bytes, cursor) {
            let mut probe = cursor + 3;
            probe = skip_whitespace(bytes, probe);
            if bytes.get(probe) == Some(&b'!') {
                probe = skip_whitespace(bytes, probe + 1);
                if bytes.get(probe) == Some(&b'[') {
                    return Some(VecMacroLocation {
                        name_start: cursor,
                        open_bracket: probe,
                    });
                }
            }
        }

        cursor += 1;
    }

    None
}

fn is_vec_ident_at(bytes: &[u8], cursor: usize) -> bool {
    bytes.get(cursor..cursor + 3) == Some(b"vec")
        && !bytes
            .get(cursor.wrapping_sub(1))
            .is_some_and(|byte| is_ident_byte(*byte))
        && !bytes
            .get(cursor + 3)
            .is_some_and(|byte| is_ident_byte(*byte))
}

fn is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn matching_bracket_end(bytes: &[u8], open_bracket: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open_bracket;

    while cursor < bytes.len() {
        if let Some(next_cursor) = skip_literal_or_comment(bytes, cursor) {
            cursor = next_cursor;
            continue;
        }

        match bytes[cursor] {
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(cursor + 1);
                }
            }
            _ => {}
        }

        cursor += 1;
    }

    None
}

fn skip_literal_or_comment(bytes: &[u8], cursor: usize) -> Option<usize> {
    if bytes[cursor] == b'/' && cursor + 1 < bytes.len() {
        if bytes[cursor + 1] == b'/' {
            return Some(skip_line_comment(bytes, cursor + 2));
        }

        if bytes[cursor + 1] == b'*' {
            return Some(skip_block_comment(bytes, cursor + 2));
        }
    }

    if bytes[cursor] == b'\'' {
        return Some(skip_char_literal(bytes, cursor + 1));
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
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            return cursor + 2;
        }
        cursor += 1;
    }

    bytes.len()
}

fn skip_char_literal(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'\'' => return cursor + 1,
            _ => cursor += 1,
        }
    }

    bytes.len()
}

fn line_prefix(text: &str, offset: usize) -> String {
    let line_start = text[..offset].rfind('\n').map_or(0, |idx| idx + 1);
    text[line_start..offset]
        .chars()
        .map(|ch| if ch == '\t' { '\t' } else { ' ' })
        .collect()
}

fn indent_subsequent_lines(text: &str, prefix: &str) -> String {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };

    let mut output = first.to_string();
    for line in lines {
        output.push('\n');
        output.push_str(prefix);
        output.push_str(line);
    }
    output
}

fn extract_wrapped_expr(
    source: &str,
    wrapper_prefix: &str,
    invalid_wrapper_message: &str,
) -> Result<String, FormatError> {
    let body = source
        .strip_prefix(wrapper_prefix)
        .and_then(|rest| rest.strip_suffix(" };\n"))
        .or_else(|| {
            source
                .strip_prefix(wrapper_prefix)
                .and_then(|rest| rest.strip_suffix(" };"))
        })
        .ok_or_else(|| FormatError::new(invalid_wrapper_message))?;

    Ok(body.to_string())
}

pub(crate) fn skip_prefixed_string(bytes: &[u8], idx: usize) -> Option<usize> {
    if bytes[idx] == b'b' {
        if idx + 1 < bytes.len() && bytes[idx + 1] == b'"' {
            return Some(skip_quoted_string(bytes, idx + 2));
        }

        if let Some(next_idx) = skip_raw_string(bytes, idx + 1) {
            return Some(next_idx);
        }
    }

    skip_raw_string(bytes, idx)
}

fn skip_raw_string(bytes: &[u8], idx: usize) -> Option<usize> {
    if bytes.get(idx) != Some(&b'r') {
        return None;
    }

    let mut cursor = idx + 1;
    let mut hashes = 0;

    while cursor < bytes.len() && bytes[cursor] == b'#' {
        hashes += 1;
        cursor += 1;
    }

    if cursor >= bytes.len() || bytes[cursor] != b'"' {
        return None;
    }

    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"' && raw_string_closes(bytes, cursor + 1, hashes) {
            return Some(cursor + hashes + 1);
        }
        cursor += 1;
    }

    Some(bytes.len())
}

fn raw_string_closes(bytes: &[u8], idx: usize, hashes: usize) -> bool {
    if idx + hashes > bytes.len() {
        return false;
    }

    bytes[idx..idx + hashes].iter().all(|byte| *byte == b'#')
}

pub(crate) fn skip_quoted_string(bytes: &[u8], mut idx: usize) -> usize {
    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' => idx += 2,
            b'"' => return idx + 1,
            _ => idx += 1,
        }
    }

    bytes.len()
}
