use syn::parse::discouraged::Speculative;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, Pat, Path, Result, Token, braced};

#[derive(Clone)]
pub(crate) struct RsxInput {
    pub(crate) root: NodeOrExpr,
}

impl Parse for RsxInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let root = input.parse()?;
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after rsx root"));
        }
        Ok(Self { root })
    }
}

#[derive(Clone)]
pub(crate) enum NodeOrExpr {
    Node(Box<Node>),
    Expr(Box<Expr>),
}

impl Parse for NodeOrExpr {
    fn parse(input: ParseStream) -> Result<Self> {
        if looks_like_node(input) {
            Ok(Self::Node(Box::new(input.parse()?)))
        } else {
            Ok(Self::Expr(Box::new(input.parse()?)))
        }
    }
}

#[derive(Clone)]
pub(crate) struct Node {
    pub(crate) path: Path,
    pub(crate) entries: Vec<Entry>,
}

#[derive(Clone)]
pub(crate) enum Entry {
    Prop(Ident, Box<NodeOrExpr>),
    Child(Box<Child>),
}

#[derive(Clone)]
pub(crate) enum Child {
    Node(Box<Node>),
    Expr(Box<Expr>),
    For(Box<ForChild>),
    If(Box<IfChild>),
}

#[derive(Clone)]
pub(crate) struct ForChild {
    pub(crate) pat: Pat,
    pub(crate) expr: Expr,
    pub(crate) body: Vec<Child>,
}

#[derive(Clone)]
pub(crate) struct IfChild {
    pub(crate) cond: Expr,
    pub(crate) then_body: Vec<Child>,
    pub(crate) else_body: Option<Vec<Child>>,
}

impl Parse for Node {
    fn parse(input: ParseStream) -> Result<Self> {
        let path: Path = input.parse()?;

        let content;
        braced!(content in input);

        let entries = parse_entries(&content)?;
        Ok(Self { path, entries })
    }
}

impl Parse for Child {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(Token![for]) {
            Ok(Self::For(Box::new(input.parse()?)))
        } else if input.peek(Token![if]) {
            Ok(Self::If(Box::new(input.parse()?)))
        } else if looks_like_node(input) {
            Ok(Self::Node(Box::new(input.parse()?)))
        } else {
            Ok(Self::Expr(Box::new(input.parse()?)))
        }
    }
}

impl Parse for ForChild {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<Token![for]>()?;
        let pat: Pat = input.call(Pat::parse_single)?;
        input.parse::<Token![in]>()?;
        let expr = input.call(Expr::parse_without_eager_brace)?;

        let content;
        braced!(content in input);
        let body = parse_children(&content)?;

        Ok(Self { pat, expr, body })
    }
}

impl Parse for IfChild {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<Token![if]>()?;
        let cond = input.call(Expr::parse_without_eager_brace)?;

        let then_content;
        braced!(then_content in input);
        let then_body = parse_children(&then_content)?;

        let else_body = if input.peek(Token![else]) {
            input.parse::<Token![else]>()?;

            if input.peek(Token![if]) {
                let inner_if: IfChild = input.parse()?;
                Some(vec![Child::If(Box::new(inner_if))])
            } else {
                let else_content;
                braced!(else_content in input);
                Some(parse_children(&else_content)?)
            }
        } else {
            None
        };

        Ok(Self {
            cond,
            then_body,
            else_body,
        })
    }
}

fn looks_like_prop(input: ParseStream<'_>) -> bool {
    if !input.peek(Ident) {
        return false;
    }

    let fork = input.fork();
    if fork.parse::<Ident>().is_err() {
        return false;
    }

    if fork.peek(Token![::]) {
        return false;
    }

    fork.peek(Token![:])
}

fn parse_entries(input: ParseStream) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();

    while !input.is_empty() {
        if looks_like_prop(input) {
            let name: Ident = input.parse()?;
            let name_str = name.to_string();

            input.parse::<Token![:]>()?;

            if name_str == "child" || name_str == "children" {
                let _value = parse_prop_value(input)?;
                return Err(syn::Error::new(
                    name.span(),
                    format!("don't use `{}:` in rsx! - add children directly", name_str),
                ));
            }

            let value = parse_prop_value(input)?;
            entries.push(Entry::Prop(name, Box::new(value)));
        } else {
            let child: Child = input.parse()?;
            entries.push(Entry::Child(Box::new(child)));
        }

        if !input.is_empty() {
            if !input.peek(Token![,]) && !input.peek(Token![;]) {
                return Err(syn::Error::new(
                    input.span(),
                    "expected `,` or `;` between entries in rsx! macro",
                ));
            }
            let _ = input.parse::<Option<Token![,]>>()?;
            let _ = input.parse::<Option<Token![;]>>()?;
        }
    }

    Ok(entries)
}

fn parse_prop_value(input: ParseStream) -> Result<NodeOrExpr> {
    let fork = input.fork();
    if let Ok(expr) = fork.parse::<Expr>() {
        input.advance_to(&fork);
        return Ok(NodeOrExpr::Expr(Box::new(expr)));
    }

    if looks_like_node(input) {
        return Ok(NodeOrExpr::Node(Box::new(input.parse()?)));
    }

    Err(input.error("expected property value"))
}

fn parse_children(input: ParseStream) -> Result<Vec<Child>> {
    let mut out = Vec::new();

    while !input.is_empty() {
        out.push(input.parse()?);

        if !input.is_empty() {
            if !input.peek(Token![,]) && !input.peek(Token![;]) {
                return Err(syn::Error::new(
                    input.span(),
                    "expected `,` or `;` between children in rsx! macro",
                ));
            }
            let _ = input.parse::<Option<Token![,]>>()?;
            let _ = input.parse::<Option<Token![;]>>()?;
        }
    }

    Ok(out)
}

fn looks_like_node(input: ParseStream<'_>) -> bool {
    let fork = input.fork();
    if fork.parse::<Path>().is_err() {
        return false;
    }
    fork.peek(syn::token::Brace)
}
