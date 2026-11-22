use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;

use crate::child_block::ChildBlock;
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
                let node = format_ident!("__ui_node", span = Span::mixed_site());
                let mut block = ChildBlock::new(lipan, &node);
                let stmts: Vec<_> = children
                    .iter()
                    .map(|c| c.expand_child(&mut block))
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

    fn expand_child(&self, block: &mut ChildBlock<'_>) -> TokenStream2 {
        match self {
            Self::Parent { expr, .. } | Self::Leaf(expr, _) => {
                block.append(expr.span(), self.expand(block.lipan))
            }
            Self::For { pat, iter, body } => {
                let stmts = block.nested(body, Self::expand_child);
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
                let then_stmts = block.nested(then_body, Self::expand_child);

                if let Some(else_body) = else_body {
                    let else_stmts = block.nested(else_body, Self::expand_child);
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
