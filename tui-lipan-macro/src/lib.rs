use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Ident, parse_macro_input};

mod rsx_ast;
mod rsx_expand;
mod ui_ast;
mod ui_expand;

use crate::rsx_ast::RsxInput;
use crate::ui_ast::UiInput;

#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RsxInput);
    let lipan = lipan_crate_path();
    input.expand(&lipan).into()
}

/// Autocomplete-friendly alternative to [`rsx!`].
///
/// Uses standard Rust builder chains with `=> { children }` sugar:
///
/// ```ignore
/// ui! {
///     Frame::new().title("Panel").border(true) => {
///         Text::new("Hello"),
///         Button::new("Click").on_click(handler),
///     }
/// }
/// ```
///
/// Everything before `=>` is a normal Rust expression (full autocomplete).
/// `=> { child1, child2, ... }` desugars to `.child(child1).child(child2)...`.
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as UiInput);
    let lipan = lipan_crate_path();
    input.expand(&lipan).into()
}

fn lipan_crate_path() -> TokenStream2 {
    match crate_name("tui-lipan") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
        Err(_) => quote!(::tui_lipan),
    }
}
