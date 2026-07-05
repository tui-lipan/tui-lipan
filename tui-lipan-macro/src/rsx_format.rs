use proc_macro2::{LineColumn, TokenStream as TokenStream2};
use quote::quote;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Macro};

use crate::format_common::{
    FormatError, NestedMacroHooks, contains_comments, format_expr, format_pat, indent_block,
    line_starts, offset_for,
};
use crate::rsx_ast::{Child, Entry, IfChild, Node, NodeOrExpr, RsxInput};

const INDENT: &str = "    ";
const INLINE_WIDTH: usize = 88;
const EXPR_WRAPPER_PREFIX: &str = "const _: () = { ";
const INVALID_NESTED_RSX_WRAPPER: &str = "invalid nested rsx wrapper";

const RSX_NESTED_HOOKS: NestedMacroHooks = NestedMacroHooks {
    contains_target_macro: expr_contains_rsx_macro,
    format_wrapped_file: format_file_contents,
    wrapper_prefix: EXPR_WRAPPER_PREFIX,
    invalid_wrapper_message: INVALID_NESTED_RSX_WRAPPER,
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
}

#[derive(Default)]
struct RsxMacroCollector {
    macros: Vec<LocatedMacro>,
}

impl<'ast> Visit<'ast> for RsxMacroCollector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if is_rsx_macro(mac) {
            let span = mac.span();
            self.macros.push(LocatedMacro {
                path: mac.path.clone(),
                tokens: mac.tokens.clone(),
                start: span.start(),
                end: span.end(),
            });
        }

        visit::visit_macro(self, mac);
    }
}

#[derive(Default)]
struct RsxExprDetector {
    found: bool,
}

impl<'ast> Visit<'ast> for RsxExprDetector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if is_rsx_macro(mac) {
            self.found = true;
            return;
        }

        visit::visit_macro(self, mac);
    }
}

fn is_rsx_macro(mac: &Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "rsx")
}

fn expr_contains_rsx_macro(expr: &Expr) -> bool {
    let mut detector = RsxExprDetector::default();
    detector.visit_expr(expr);
    detector.found
}

pub(crate) fn format_file_contents(source: &str) -> Result<Option<String>, FormatError> {
    let file = syn::parse_file(source)?;
    let mut collector = RsxMacroCollector::default();
    collector.visit_file(&file);

    if collector.macros.is_empty() {
        return Ok(None);
    }

    let line_starts = line_starts(source);
    let mut replacements = Vec::with_capacity(collector.macros.len());

    for mac in collector.macros {
        let start = offset_for(&line_starts, mac.start)?;
        let end = offset_for(&line_starts, mac.end)?;

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
            return Err(FormatError::new("overlapping rsx! macro spans"));
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

#[cfg(test)]
pub(crate) fn format_macro_source(source: &str) -> Result<String, FormatError> {
    let input: RsxInput = syn::parse_str(source)?;
    Ok(format_node_or_expr(&input.root)?.text)
}

fn format_macro_invocation(
    path: &syn::Path,
    tokens: &TokenStream2,
    line_indent: &str,
) -> Result<String, FormatError> {
    let input: RsxInput = syn::parse2(tokens.clone())?;
    let path_text = quote!(#path).to_string();
    let body = format_node_or_expr(&input.root)?.text;

    if !body.contains('\n') && path_text.len() + body.len() + 6 <= INLINE_WIDTH {
        return Ok(format!("{path_text}! {{ {body} }}"));
    }

    let mut output = String::new();
    output.push_str(&path_text);
    output.push_str("! {\n");
    output.push_str(&indent_block(&body, &format!("{line_indent}{INDENT}")));
    output.push('\n');
    output.push_str(line_indent);
    output.push('}');
    Ok(output)
}

fn format_node_or_expr(value: &NodeOrExpr) -> Result<Rendered, FormatError> {
    match value {
        NodeOrExpr::Node(node) => Ok(format_node(node)?),
        NodeOrExpr::Expr(expr) => Ok(Rendered::new(format_expr(expr, Some(&RSX_NESTED_HOOKS))?)),
    }
}

fn format_node(node: &Node) -> Result<Rendered, FormatError> {
    let path_tokens = &node.path;
    let path = quote!(#path_tokens).to_string();

    if node.entries.is_empty() {
        return Ok(Rendered::new(format!("{path} {{}}")));
    }

    let entries = node
        .entries
        .iter()
        .map(format_entry)
        .collect::<Result<Vec<_>, _>>()?;
    let child_count = node
        .entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Child(_)))
        .count();

    if child_count == 0 && entries.len() == 1 && !entries[0].multiline {
        let candidate = format!("{path} {{ {} }}", entries[0].text);
        if candidate.len() <= INLINE_WIDTH {
            return Ok(Rendered::new(candidate));
        }
    }

    let body = entries
        .iter()
        .map(|entry| indent_block(&format!("{},", entry.text), INDENT))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Rendered::new(format!("{path} {{\n{body}\n}}")))
}

fn format_entry(entry: &Entry) -> Result<Rendered, FormatError> {
    match entry {
        Entry::Prop(name, value) => {
            let value = format_node_or_expr(value)?;
            Ok(Rendered::new(format!("{}: {}", name, value.text)))
        }
        Entry::Child(child) => format_child(child),
    }
}

fn format_child(child: &Child) -> Result<Rendered, FormatError> {
    match child {
        Child::Node(node) => format_node(node),
        Child::Expr(expr) => Ok(Rendered::new(format_expr(expr, Some(&RSX_NESTED_HOOKS))?)),
        Child::For(for_child) => {
            let pat = format_pat(&for_child.pat);
            let expr = format_expr(&for_child.expr, Some(&RSX_NESTED_HOOKS))?;
            let body = format_child_block(&for_child.body)?;
            Ok(Rendered::new(match body {
                Some(body) => format!("for {pat} in {expr} {{\n{body}\n}}"),
                None => format!("for {pat} in {expr} {{}}"),
            }))
        }
        Child::If(if_child) => format_if_child(if_child),
    }
}

fn format_if_child(if_child: &IfChild) -> Result<Rendered, FormatError> {
    let cond = format_expr(&if_child.cond, Some(&RSX_NESTED_HOOKS))?;
    let then_body = format_child_block(&if_child.then_body)?;

    let mut output = match then_body {
        Some(body) => format!("if {cond} {{\n{body}\n}}"),
        None => format!("if {cond} {{}}"),
    };

    if let Some(else_body) = &if_child.else_body {
        if let [Child::If(inner_if)] = else_body.as_slice() {
            output.push_str(" else ");
            output.push_str(&format_if_child(inner_if)?.text);
        } else if let Some(body) = format_child_block(else_body)? {
            output.push_str(&format!(" else {{\n{body}\n}}"));
        } else {
            output.push_str(" else {}");
        }
    }

    Ok(Rendered::new(output))
}

fn format_child_block(children: &[Child]) -> Result<Option<String>, FormatError> {
    if children.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        children
            .iter()
            .map(format_child)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|child| indent_block(&format!("{},", child.text), INDENT))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
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

#[cfg(test)]
mod tests {
    use super::{format_file_contents, format_macro_source};

    #[test]
    fn formats_simple_tree() {
        let formatted = format_macro_source(
            r#"VStack{gap:1,Text{content:"Hello"},Button{label:"Save",on_click:ctx.link().callback(|_| Msg::Save)}}"#,
        )
        .unwrap();

        assert_eq!(
            formatted,
            r#"VStack {
    gap: 1,
    Text { content: "Hello" },
    Button {
        label: "Save",
        on_click: ctx.link().callback(|_| Msg::Save),
    },
}"#
        );
    }

    #[test]
    fn formats_control_flow() {
        let formatted = format_macro_source(
            r#"VStack{if let Some(error)=&state.error{Text{content:error.clone()}}else if state.loading{Spinner{label:"Loading"}}}"#,
        )
        .unwrap();

        assert_eq!(
            formatted,
            r#"VStack {
    if let Some(error) = &state.error {
        Text { content: error.clone() },
    } else if state.loading {
        Spinner { label: "Loading" },
    },
}"#
        );
    }

    #[test]
    fn rewrites_rsxs_inside_rust_files() {
        let source = r#"fn view() {
    let body = rsx! { VStack{gap:1,Text{content:"Hello"}} };
    let footer = rsx! { Text{content:format!("count = {}", count)} };
}
"#;

        let formatted = format_file_contents(source).unwrap().unwrap();

        assert_eq!(
            formatted,
            r#"fn view() {
    let body = rsx! {
        VStack {
            gap: 1,
            Text { content: "Hello" },
        }
    };
    let footer = rsx! { Text { content: format!("count = {}", count) } };
}
"#
        );
    }

    #[test]
    fn formats_nested_loops_and_match_children() {
        let formatted = format_macro_source(
            r#"VStack{for item in items.iter(){if item.visible{match item.kind{ItemKind::Primary=>Button{label:item.label.clone()},ItemKind::Secondary=>Text{content:item.label.clone()},}}}}"#,
        )
        .unwrap();

        assert_eq!(
            formatted,
            r#"VStack {
    for item in items.iter() {
        if item.visible {
            match item.kind {
                ItemKind::Primary => {
                    Button {
                        label: item.label.clone(),
                    }
                }
                ItemKind::Secondary => {
                    Text {
                        content: item.label.clone(),
                    }
                }
            },
        },
    },
}"#
        );
    }

    #[test]
    fn formats_long_callbacks() {
        let formatted = format_macro_source(
            r#"Button{label:"Save",on_click:ctx.link().callback(|event| Msg::SaveRequested{force:event.shift, target:ctx.props.target.clone(),})}"#,
        )
        .unwrap();

        assert_eq!(
            formatted,
            r#"Button {
    label: "Save",
    on_click: ctx.link()
        .callback(|event| Msg::SaveRequested {
            force: event.shift,
            target: ctx.props.target.clone(),
        }),
}"#
        );
    }

    #[test]
    fn formats_vec_prop_with_long_method_chains() {
        let formatted = format_macro_source(
            r#"Frame{decorations:vec![EdgeDecoration::new(Edge::Bottom).glyph(DecorationGlyph::HalfBlock).style(Style::new().fg(input_bg).bg(ThemeColors::modal_backdrop())).placement(DecorationPlacement::Outside),EdgeDecoration::new(Edge::Left).glyph(DecorationGlyph::AutoBlock).style(Style::new().fg(accent_color).bg(ThemeColors::modal_backdrop())).cap_end(DecorationGlyph::CapBottom)],}"#,
        )
        .unwrap();

        assert_eq!(
            formatted,
            r#"Frame {
    decorations: vec![
        EdgeDecoration::new(Edge::Bottom)
            .glyph(DecorationGlyph::HalfBlock)
            .style(Style::new().fg(input_bg).bg(ThemeColors::modal_backdrop()))
            .placement(DecorationPlacement::Outside),
        EdgeDecoration::new(Edge::Left)
            .glyph(DecorationGlyph::AutoBlock)
            .style(Style::new().fg(accent_color).bg(ThemeColors::modal_backdrop()))
            .cap_end(DecorationGlyph::CapBottom),
    ],
}"#
        );
    }

    #[test]
    fn skips_macros_with_comments() {
        let source = r#"fn view() {
    let commented = rsx! {
        VStack {
            // Keep this explanation with the block.
            Text { content: "Hello" }
        }
    };
    let clean = rsx! { Text{content:"World"} };
}
"#;

        let formatted = format_file_contents(source).unwrap().unwrap();

        assert_eq!(
            formatted,
            r#"fn view() {
    let commented = rsx! {
        VStack {
            // Keep this explanation with the block.
            Text { content: "Hello" }
        }
    };
    let clean = rsx! { Text { content: "World" } };
}
"#
        );
    }

    #[test]
    fn recursively_formats_nested_rsx_macros_inside_expressions() {
        let source = r#"fn view() {
    let body = rsx! {
        VStack {
            match foo {
                Kind::A => {
                    rsx! { Frame{border:true,TextArea{value:text.clone(),wrap:true}} }
                }
                Kind::B => {
                    Text { content: "ok" }
                }
            }
        }
    };
}
"#;

        let formatted = format_file_contents(source).unwrap().unwrap();

        assert_eq!(
            formatted,
            r#"fn view() {
    let body = rsx! {
        VStack {
            match foo {
                Kind::A => {
                    rsx! {
                        Frame {
                            border: true,
                            TextArea {
                                value: text.clone(),
                                wrap: true,
                            },
                        }
                    }
                }
                Kind::B => Text { content: "ok" },
            },
        }
    };
}
"#
        );
    }
}
