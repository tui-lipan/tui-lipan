use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Expr, Ident, Result};

use crate::child_block::ChildBlock;
use crate::rsx_ast::{Child, Entry, IfChild, Node, NodeOrExpr, RsxInput};

impl RsxInput {
    pub(crate) fn expand(self, lipan: &TokenStream2) -> TokenStream2 {
        let root = self.root.expand_value(lipan);
        quote! { ::core::convert::Into::<#lipan::Element>::into(#root) }
    }
}

impl NodeOrExpr {
    fn expand_value(&self, lipan: &TokenStream2) -> TokenStream2 {
        match self {
            Self::Node(node) => node.expand_value(lipan),
            Self::Expr(expr) => quote! { #expr },
        }
    }
}

impl Node {
    fn type_name(&self) -> String {
        self.path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default()
    }

    fn expand_value(&self, lipan: &TokenStream2) -> TokenStream2 {
        match self.expand_value_impl(lipan) {
            Ok(tokens) => tokens,
            Err(err) => err.to_compile_error(),
        }
    }

    fn expand_value_impl(&self, lipan: &TokenStream2) -> Result<TokenStream2> {
        let ty = self.type_name();
        let constructor_key = CONSTRUCTOR_KEYS
            .iter()
            .find_map(|&(widget, key)| (widget == ty).then_some(key));

        let mut constructor_arg = None;
        let mut key_expr: Option<(Ident, Expr)> = None;
        let mut constraint_exprs: Vec<(Ident, Expr)> = Vec::new();

        let node_ident = format_ident!("__rsx_node", span = Span::mixed_site());
        let mut block = ChildBlock::new(lipan, &node_ident);
        let mut stmts = Vec::new();

        for entry in &self.entries {
            match entry {
                Entry::Prop(name, value) => {
                    let name_str = name.to_string();

                    // A list item is not an element; its key is one of its own props.
                    if name_str == "key" && ty != "ListItem" {
                        let NodeOrExpr::Expr(expr) = value.as_ref() else {
                            return Err(syn::Error::new(
                                name.span(),
                                "`key:` must be a Rust expression",
                            ));
                        };
                        key_expr = Some((name.clone(), expr.as_ref().clone()));
                        continue;
                    }

                    if is_element_constraint(&name_str) {
                        let NodeOrExpr::Expr(expr) = value.as_ref() else {
                            return Err(syn::Error::new(
                                name.span(),
                                format!("`{}:` must be a Rust expression", name_str),
                            ));
                        };
                        constraint_exprs.push((name.clone(), expr.as_ref().clone()));
                        continue;
                    }

                    let value = value.expand_value(lipan);
                    if constructor_key == Some(name_str.as_str()) && constructor_arg.is_none() {
                        constructor_arg = Some(value);
                    } else {
                        stmts.push(quote! { #node_ident = #node_ident.#name(#value); });
                    }
                }
                Entry::Child(child) => stmts.push(child.expand_apply(&mut block)),
            }
        }

        let path = &self.path;
        if let Some(key) = constructor_key
            && constructor_arg.is_none()
        {
            return Err(syn::Error::new_spanned(
                path,
                format!("`{ty}` requires `{key}: ...` property"),
            ));
        }

        let builder = if ty == "AccordionItem" {
            // The declared child replaces the constructor's initial content.
            quote! { #path::new(#constructor_arg, #lipan::prelude::Spacer::new()) }
        } else {
            quote! { #path::new(#constructor_arg) }
        };
        let finish = finish_node(lipan, &ty, &node_ident, key_expr, constraint_exprs)?;
        Ok(quote! {{
            let mut #node_ident = #builder;
            #(#stmts)*
            #finish
        }})
    }
}

fn finish_node(
    lipan: &TokenStream2,
    ty: &str,
    node: &Ident,
    key: Option<(Ident, Expr)>,
    constraints: Vec<(Ident, Expr)>,
) -> Result<TokenStream2> {
    if key.is_none() && constraints.is_empty() {
        return Ok(quote! { #node });
    }

    let mut chain = quote! { ::core::convert::Into::<#lipan::Element>::into(#node) };
    for (name, expr) in constraints {
        chain = quote! { #chain.#name(#expr) };
    }

    if let Some((ident, expr)) = key {
        if ty == "Tab" {
            return Err(syn::Error::new(
                ident.span(),
                format!("`key:` is not supported for `{ty}`"),
            ));
        }
        chain = quote! { #chain.key(#expr) };
    }
    Ok(chain)
}

impl Child {
    fn expand_apply(&self, block: &mut ChildBlock<'_>) -> TokenStream2 {
        match self {
            Self::Node(node) => block.append(node.path.span(), node.expand_value(block.lipan)),
            Self::Expr(expr) => block.append(expr.span(), quote! { #expr }),
            Self::For(for_child) => {
                let pat = &for_child.pat;
                let expr = &for_child.expr;
                let body = block.nested(&for_child.body, Self::expand_apply);

                quote! {
                    for #pat in #expr {
                        #(#body)*
                    }
                }
            }
            Self::If(if_child) => expand_if_child(if_child, block),
        }
    }
}

fn expand_if_child(if_child: &IfChild, block: &ChildBlock<'_>) -> TokenStream2 {
    let cond = &if_child.cond;
    let then_body = block.nested(&if_child.then_body, Child::expand_apply);
    let else_clause = if_child.else_body.as_ref().map(|else_body| {
        let body = block.nested(else_body, Child::expand_apply);
        quote! { else { #(#body)* } }
    });

    quote! {
        if #cond {
            #(#then_body)*
        } #else_clause
    }
}

fn is_element_constraint(name: &str) -> bool {
    matches!(
        name,
        "min_width" | "max_width" | "min_height" | "max_height" | "automation_id"
    )
}

const CONSTRUCTOR_KEYS: &[(&str, &str)] = &[
    ("AccordionItem", "title"),
    ("AsciiCanvas", "lines"),
    ("Badge", "content"),
    ("Button", "label"),
    ("Checkbox", "checked"),
    ("ContextMenu", "trigger"),
    ("ContextProvider", "value"),
    ("Divider", "orientation"),
    ("DocumentView", "value"),
    ("DraggableTab", "label"),
    ("FileTree", "root"),
    ("Flowchart", "direction"),
    ("Heatmap", "data"),
    ("HexArea", "bytes"),
    ("Image", "src"),
    ("Input", "value"),
    ("ListItem", "label"),
    ("PaginationBar", "state"),
    ("ProgressBar", "progress"),
    ("Radio", "options"),
    ("Slider", "value"),
    ("Sparkline", "data"),
    ("Splitter", "orientation"),
    ("Tab", "label"),
    ("Text", "content"),
    ("TextArea", "value"),
    ("ThemeProvider", "theme"),
    ("Toast", "content"),
    ("Tooltip", "content"),
    ("Tree", "root"),
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::CONSTRUCTOR_KEYS;

    #[test]
    fn documented_constructor_keys_match_expansion() {
        let docs = include_str!("../../../docs/macros.md");
        let (_, constructor_section) = docs.split_once("### Constructor Keys\n").unwrap();
        let table = constructor_section.split("\n### ").next().unwrap();

        let documented: BTreeSet<_> = table
            .lines()
            .filter_map(|line| {
                let mut cells = line.strip_prefix("| `")?.split("` | `");
                Some((cells.next()?, cells.next()?))
            })
            .collect();
        let implemented = CONSTRUCTOR_KEYS.iter().copied().collect();
        assert_eq!(documented, implemented);
        assert_eq!(documented.len(), CONSTRUCTOR_KEYS.len());
    }
}
