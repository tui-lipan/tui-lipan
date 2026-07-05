use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{Expr, Ident, Result};

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
        let ctor_keys = constructor_keys_for_type(&ty);

        let mut ctor_args: Vec<Option<TokenStream2>> = vec![None; ctor_keys.len()];
        let mut key_expr: Option<(Ident, Expr)> = None;
        let mut constraint_exprs: Vec<(Ident, Expr)> = Vec::new();

        let node_ident = format_ident!("__rsx_node");
        let add_method = add_method_for_parent(&ty);

        let mut stmts: Vec<TokenStream2> = Vec::new();

        let child_count = self
            .entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Child(_)))
            .count();

        if let Some(limit) = max_children_for_type(&ty)
            && child_count > limit
        {
            let hint = if limit == 0 {
                format!("`{ty}` does not accept child nodes")
            } else {
                "Note: Use VStack or HStack for multiple children".to_string()
            };
            return Err(syn::Error::new_spanned(
                &self.path,
                format!(
                    "`{}` can only have {} child{}, but {} were provided\n{}",
                    ty,
                    limit,
                    if limit == 1 { "" } else { "ren" },
                    child_count,
                    hint,
                ),
            ));
        }

        for entry in &self.entries {
            match entry {
                Entry::Prop(name, value) => {
                    let name_str = name.to_string();

                    if name_str == "key" {
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

                    if let Some(pos) = ctor_keys.iter().position(|key| *key == name_str.as_str())
                        && ctor_args[pos].is_none()
                    {
                        ctor_args[pos] = Some(value.expand_value(lipan));
                        continue;
                    }

                    let method = map_prop_method(name);
                    let value = value.expand_value(lipan);
                    stmts.push(quote! { #node_ident = #node_ident.#method(#value); });
                }
                Entry::Child(child) => {
                    stmts.push(child.expand_apply(lipan, &node_ident, &add_method)?);
                }
            }
        }

        let path = &self.path;

        let base = if !ctor_keys.is_empty() {
            let mut resolved = Vec::with_capacity(ctor_keys.len());
            for (key, arg) in ctor_keys.iter().zip(ctor_args.iter()) {
                let Some(arg) = arg else {
                    return Err(syn::Error::new_spanned(
                        path,
                        format!("`{ty}` requires `{key}: ...` property"),
                    ));
                };
                resolved.push(quote! { #arg });
            }
            quote! { #path::new(#(#resolved),*) }
        } else {
            quote! { #path::new() }
        };

        let has_element_modifiers = key_expr.is_some() || !constraint_exprs.is_empty();

        let finish = if has_element_modifiers {
            if matches!(ty.as_str(), "Tab" | "ListItem")
                && let Some((ident, _)) = &key_expr
            {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("`key:` is not supported for `{ty}`"),
                ));
            }

            let mut chain = quote! { ::core::convert::Into::<#lipan::Element>::into(#node_ident) };

            for (name, expr) in constraint_exprs {
                chain = quote! { #chain.#name(#expr) };
            }

            if let Some((_key_ident, key_expr)) = key_expr {
                chain = quote! { #chain.key(#key_expr) };
            }

            chain
        } else {
            quote! { #node_ident }
        };

        Ok(quote! {{
            let mut #node_ident = #base;
            #(#stmts)*
            #finish
        }})
    }
}

impl Child {
    fn expand_apply(
        &self,
        lipan: &TokenStream2,
        node_ident: &Ident,
        add_method: &Ident,
    ) -> Result<TokenStream2> {
        Ok(match self {
            Self::Node(node) => {
                let value = node.expand_value(lipan);
                quote! { #node_ident = #node_ident.#add_method(#value); }
            }
            Self::Expr(expr) => {
                quote! { #node_ident = #node_ident.#add_method(#expr); }
            }
            Self::For(for_child) => {
                let pat = &for_child.pat;
                let expr = &for_child.expr;

                let mut body = Vec::new();
                for child in &for_child.body {
                    body.push(child.expand_apply(lipan, node_ident, add_method)?);
                }

                quote! {
                    for #pat in #expr {
                        #(#body)*
                    }
                }
            }
            Self::If(if_child) => expand_if_child(if_child, lipan, node_ident, add_method)?,
        })
    }
}

fn expand_if_child(
    if_child: &IfChild,
    lipan: &TokenStream2,
    node_ident: &Ident,
    add_method: &Ident,
) -> Result<TokenStream2> {
    let cond = &if_child.cond;

    let mut then_body = Vec::new();
    for child in &if_child.then_body {
        then_body.push(child.expand_apply(lipan, node_ident, add_method)?);
    }

    Ok(if let Some(else_body) = &if_child.else_body {
        let mut else_tokens = Vec::new();
        for child in else_body {
            else_tokens.push(child.expand_apply(lipan, node_ident, add_method)?);
        }

        quote! {
            if #cond {
                #(#then_body)*
            } else {
                #(#else_tokens)*
            }
        }
    } else {
        quote! {
            if #cond {
                #(#then_body)*
            }
        }
    })
}

fn map_prop_method(name: &Ident) -> Ident {
    match name.to_string().as_str() {
        "alignment" => Ident::new("align", name.span()),
        "spacing" => Ident::new("gap", name.span()),
        _ => name.clone(),
    }
}

fn add_method_for_parent(parent_ty: &str) -> Ident {
    match parent_ty {
        "Tabs" => Ident::new("tab", Span::call_site()),
        "List" => Ident::new("item", Span::call_site()),
        "Accordion" => Ident::new("item", Span::call_site()),
        "DraggableTabBar" => Ident::new("tab", Span::call_site()),
        "AccordionItem" => Ident::new("content", Span::call_site()),
        _ => Ident::new("child", Span::call_site()),
    }
}

fn is_element_constraint(name: &str) -> bool {
    matches!(
        name,
        "min_width" | "max_width" | "min_height" | "max_height"
    )
}

fn constructor_keys_for_type(ty: &str) -> &'static [&'static str] {
    match ty {
        "Text" => &["content"],
        "Button" => &["label"],
        "Input" => &["value"],
        "Tab" => &["label"],
        "ListItem" => &["text"],
        "Checkbox" => &["checked"],
        "ProgressBar" => &["progress"],
        "TextArea" => &["value"],
        "Radio" => &["options"],
        "Grid" => &["columns"],
        "Heatmap" => &["data"],
        "Sparkline" => &["data"],
        "Tree" => &["root"],
        "FileTree" => &["root"],
        "Modal" => &[],
        "Toast" => &["message"],
        "Tooltip" => &["text"],
        "ThemeProvider" => &["theme"],
        "Badge" => &["content"],
        "Slider" => &["value"],
        "Image" => &["src"],
        "Divider" => &["orientation"],
        "Splitter" => &["orientation"],
        "AsciiCanvas" => &["lines"],
        "AccordionItem" => &["title"],
        "ContextMenu" => &["trigger"],
        "DocumentView" => &["value"],
        "DraggableTab" => &["label"],
        "Flowchart" => &["direction"],
        "HexArea" => &["bytes"],
        "PaginationBar" => &["state"],
        "DiffView" => &[],
        _ => &[],
    }
}

fn max_children_for_type(ty: &str) -> Option<usize> {
    match ty {
        "Center" | "Frame" | "Group" | "Portal" => Some(1),
        "CenterPin" => Some(0),
        "AccordionItem" | "Badge" | "DragSource" | "DropTarget" | "Modal" | "MouseRegion"
        | "ThemeProvider" | "Tooltip" => Some(1),
        "VStack" | "HStack" | "ZStack" | "List" | "Tabs" | "Grid" | "Tree" | "ScrollView"
        | "Splitter" => None,
        "Accordion" | "DraggableTabBar" | "StatusBar" => None,
        "Text" | "BigText" | "Button" | "Input" | "TextArea" | "Checkbox" | "ProgressBar"
        | "Slider" | "Spinner" | "Divider" | "Spacer" | "Tab" | "ListItem" | "LogView"
        | "Terminal" | "FileTree" | "Image" | "AsciiCanvas" => Some(0),
        "Breadcrumb" | "Chart" | "ComboBox" | "ContextMenu" | "DatePicker" | "DiffView"
        | "DocumentView" | "DraggableTab" | "Flowchart" | "Graph" | "Heatmap" | "HexArea"
        | "ManagedTerminal" | "MultiSelect" | "PaginationBar" | "Popover" | "Radio"
        | "SearchPalette" | "Select" | "SequenceDiagram" | "Sparkline" | "Table" | "Toast" => {
            Some(0)
        }
        _ => None,
    }
}
