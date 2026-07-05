use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{Expr, Ident};

use crate::ui_ast::{UiInput, UiItem};

impl UiInput {
    pub(crate) fn expand(self, lipan: &TokenStream2) -> TokenStream2 {
        let root = self.root.expand(lipan);
        quote! { ::core::convert::Into::<#lipan::Element>::into(#root) }
    }
}

impl UiItem {
    fn expand(&self, lipan: &TokenStream2) -> TokenStream2 {
        match self {
            Self::Parent {
                expr,
                key,
                children,
            } => {
                let add_method = infer_add_method(expr);
                let node = format_ident!("__ui_node");
                let stmts: Vec<_> = children
                    .iter()
                    .map(|c| c.expand_child(lipan, &node, &add_method))
                    .collect();

                let finish = if let Some(key_expr) = key {
                    quote! {
                        ::core::convert::Into::<#lipan::Element>::into(#node).key(#key_expr)
                    }
                } else {
                    quote! { #node }
                };

                quote! {{
                    let mut #node = #expr;
                    #(#stmts)*
                    #finish
                }}
            }
            Self::Leaf(expr, key) => {
                if let Some(key_expr) = key {
                    quote! {
                        ::core::convert::Into::<#lipan::Element>::into(#expr).key(#key_expr)
                    }
                } else {
                    quote! { #expr }
                }
            }
            Self::For { .. } | Self::If { .. } => {
                quote! {
                    compile_error!(
                        "`for`/`if` at root level requires a parent container with `=> { ... }`"
                    )
                }
            }
        }
    }

    fn expand_child(&self, lipan: &TokenStream2, node: &Ident, add_method: &Ident) -> TokenStream2 {
        match self {
            Self::Parent { .. } | Self::Leaf(..) => {
                let value = self.expand(lipan);
                quote! { #node = #node.#add_method(#value); }
            }
            Self::For { pat, iter, body } => {
                let stmts: Vec<_> = body
                    .iter()
                    .map(|c| c.expand_child(lipan, node, add_method))
                    .collect();
                quote! {
                    for #pat in #iter {
                        #(#stmts)*
                    }
                }
            }
            Self::If {
                cond,
                then_body,
                else_body,
            } => {
                let then_stmts: Vec<_> = then_body
                    .iter()
                    .map(|c| c.expand_child(lipan, node, add_method))
                    .collect();

                if let Some(else_body) = else_body {
                    let else_stmts: Vec<_> = else_body
                        .iter()
                        .map(|c| c.expand_child(lipan, node, add_method))
                        .collect();
                    quote! {
                        if #cond {
                            #(#then_stmts)*
                        } else {
                            #(#else_stmts)*
                        }
                    }
                } else {
                    quote! {
                        if #cond {
                            #(#then_stmts)*
                        }
                    }
                }
            }
        }
    }
}

/// Infer the correct add method (`.child()`, `.item()`, `.tab()`, etc.) by
/// extracting the widget type name from the root of a builder chain expression.
fn infer_add_method(expr: &Expr) -> Ident {
    match extract_root_type_name(expr).as_deref() {
        Some("Tabs") => Ident::new("tab", Span::call_site()),
        Some("List") => Ident::new("item", Span::call_site()),
        Some("Accordion") => Ident::new("item", Span::call_site()),
        Some("DraggableTabBar") => Ident::new("tab", Span::call_site()),
        Some("AccordionItem") => Ident::new("content", Span::call_site()),
        _ => Ident::new("child", Span::call_site()),
    }
}

/// Walk a builder chain (`Type::new().method().method()`) back to its root
/// constructor call and return the type name (the path segment before `::new`).
fn extract_root_type_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::MethodCall(mc) => extract_root_type_name(&mc.receiver),
        Expr::Call(call) => {
            if let Expr::Path(ep) = call.func.as_ref() {
                let segs = &ep.path.segments;
                if segs.len() >= 2 {
                    return Some(segs[segs.len() - 2].ident.to_string());
                }
            }
            None
        }
        Expr::Path(ep) => ep.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}
