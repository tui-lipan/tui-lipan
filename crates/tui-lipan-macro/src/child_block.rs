use proc_macro2::{Ident, Span, TokenStream};
use quote::quote_spanned;

pub(crate) struct ChildBlock<'a> {
    pub(crate) lipan: &'a TokenStream,
    parent: &'a Ident,
    has_child: bool,
}

impl<'a> ChildBlock<'a> {
    pub(crate) fn new(lipan: &'a TokenStream, parent: &'a Ident) -> Self {
        Self {
            lipan,
            parent,
            has_child: false,
        }
    }

    pub(crate) fn append(&mut self, span: Span, child: TokenStream) -> TokenStream {
        let Self { lipan, parent, .. } = *self;
        let additional = std::mem::replace(&mut self.has_child, true);
        quote_spanned! { span=>
            #parent = <_ as #lipan::UiChildren<_, #additional>>::ui_child(
                #parent, #child,
            );
        }
    }

    pub(crate) fn nested<T>(
        &self,
        children: &[T],
        expand: impl Fn(&T, &mut Self) -> TokenStream,
    ) -> Vec<TokenStream> {
        let mut block = Self::new(self.lipan, self.parent);
        children
            .iter()
            .map(|child| expand(child, &mut block))
            .collect()
    }
}
