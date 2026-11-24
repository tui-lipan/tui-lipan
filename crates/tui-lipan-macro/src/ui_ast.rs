use syn::parse::{Parse, ParseStream};
use syn::{Expr, Pat, Result, Token, braced};

pub(crate) struct UiInput {
    pub(crate) root: UiItem,
}

pub(crate) enum UiItem {
    /// `expr [@ key] => { children... }` - a widget with children attached via
    /// the appropriate add method (`.child()`, `.item()`, `.tab()`, etc.).
    Parent {
        expr: Box<Expr>,
        key: Option<Box<Expr>>,
        children: Vec<UiItem>,
    },
    /// A bare expression, optionally keyed with `@ key` - a leaf widget or any
    /// `Into<Element>` value.
    Leaf(Box<Expr>, Option<Box<Expr>>),
    /// `for pat in iter { children... }`
    For {
        pat: Box<Pat>,
        iter: Box<Expr>,
        body: Vec<UiItem>,
    },
    /// `if cond { then... } [else { else... }]`
    If {
        cond: Box<Expr>,
        then_body: Vec<UiItem>,
        else_body: Option<Vec<UiItem>>,
    },
}

impl Parse for UiInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let root = parse_ui_item(input)?;
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after ui! root"));
        }
        Ok(Self { root })
    }
}

fn parse_ui_item(input: ParseStream) -> Result<UiItem> {
    if input.peek(Token![for]) {
        return parse_for_item(input);
    }
    if input.peek(Token![if]) {
        return parse_if_item(input);
    }

    let expr = Box::new(input.call(Expr::parse_without_eager_brace)?);

    let key = if input.peek(Token![@]) {
        input.parse::<Token![@]>()?;
        Some(Box::new(input.call(Expr::parse_without_eager_brace)?))
    } else {
        None
    };

    if input.peek(Token![=>]) {
        input.parse::<Token![=>]>()?;
        let content;
        braced!(content in input);
        let children = parse_ui_children(&content)?;
        return Ok(UiItem::Parent {
            expr,
            key,
            children,
        });
    }

    Ok(UiItem::Leaf(expr, key))
}

fn parse_for_item(input: ParseStream) -> Result<UiItem> {
    input.parse::<Token![for]>()?;
    let pat = Box::new(input.call(Pat::parse_single)?);
    input.parse::<Token![in]>()?;
    let iter = Box::new(input.call(Expr::parse_without_eager_brace)?);

    let content;
    braced!(content in input);
    let body = parse_ui_children(&content)?;

    Ok(UiItem::For { pat, iter, body })
}

fn parse_if_item(input: ParseStream) -> Result<UiItem> {
    input.parse::<Token![if]>()?;
    let cond = Box::new(input.call(Expr::parse_without_eager_brace)?);

    let then_content;
    braced!(then_content in input);
    let then_body = parse_ui_children(&then_content)?;

    let else_body = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        if input.peek(Token![if]) {
            Some(vec![parse_if_item(input)?])
        } else {
            let else_content;
            braced!(else_content in input);
            Some(parse_ui_children(&else_content)?)
        }
    } else {
        None
    };

    Ok(UiItem::If {
        cond,
        then_body,
        else_body,
    })
}

pub(crate) fn parse_ui_children(input: ParseStream) -> Result<Vec<UiItem>> {
    let mut items = Vec::new();

    while !input.is_empty() {
        items.push(parse_ui_item(input)?);

        if !input.is_empty() {
            if !input.peek(Token![,]) && !input.peek(Token![;]) {
                return Err(syn::Error::new(
                    input.span(),
                    "expected `,` or `;` between items in ui! macro",
                ));
            }
            let _ = input.parse::<Option<Token![,]>>()?;
            let _ = input.parse::<Option<Token![;]>>()?;
        }
    }

    Ok(items)
}
