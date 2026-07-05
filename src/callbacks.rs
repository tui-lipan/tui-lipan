//! Ready-made [`Callback`] values for common side effects, such as opening URLs without a `Msg` route.

use crate::callback::Callback;
use crate::utils::open_url;
use crate::widgets::{DocumentClickEvent, HyperlinkEvent};

/// Returns a callback that opens [`HyperlinkEvent::href`] with [`open_url()`], ignoring errors.
pub fn open_hyperlink() -> Callback<HyperlinkEvent> {
    Callback::new(|ev: HyperlinkEvent| {
        if let Some(href) = ev.href.as_deref() {
            let _ = open_url(href);
        }
    })
}

/// Returns a callback that opens [`DocumentClickEvent::link`] with [`open_url()`], ignoring errors.
pub fn open_document_link() -> Callback<DocumentClickEvent> {
    Callback::new(|ev: DocumentClickEvent| {
        if let Some(url) = ev.link.as_deref() {
            let _ = open_url(url);
        }
    })
}
