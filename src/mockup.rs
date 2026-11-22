//! Zero-boilerplate layout prototyping.
//!
//! Use [`Mockup`] or the `mockup!` macro to preview a UI layout
//! without defining `Message`, `State`, or `update()` logic.
//!
//! # Example
//!
//! ```rust,no_run
//! use tui_lipan::prelude::*;
//!
//! fn main() -> Result<()> {
//!     mockup!("Dashboard", {
//!         Frame::new()
//!             .title("Panel")
//!             .border(true)
//!             .child(Text::new("Hello!"))
//!     })
//! }
//! ```

use crate::core::component::{Component, Context, KeyUpdate, Update};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent};

/// A view-only component wrapper for rapid UI prototyping.
///
/// `Mockup` wraps a closure that returns an [`Element`] tree and provides
/// the full [`Component`] implementation automatically. No messages,
/// no state, no `update()` - just the view.
///
/// Press `Esc` or `q` to quit.
///
/// # Usage
///
/// Use directly with `App`:
///
/// ```rust,no_run
/// use tui_lipan::prelude::*;
///
/// fn main() -> Result<()> {
///     App::new()
///         .title("My Layout")
///         .mount(Mockup::new(|| {
///             Frame::new()
///                 .title("Hello")
///                 .border(true)
///                 .child(Text::new("World"))
///                 .into()
///         }))
///         .run()
/// }
/// ```
///
/// Or use the `mockup!` macro for even less boilerplate:
///
/// ```rust,no_run
/// use tui_lipan::prelude::*;
///
/// fn main() -> Result<()> {
///     mockup!("My Layout", {
///         Frame::new()
///             .title("Hello")
///             .border(true)
///             .child(Text::new("World"))
///     })
/// }
/// ```
pub struct Mockup<F> {
    view_fn: F,
}

impl<F> Mockup<F>
where
    F: Fn() -> Element + 'static,
{
    /// Create a new mockup from a closure that builds the UI.
    pub fn new(view_fn: F) -> Self {
        Self { view_fn }
    }
}

impl<F> Component for Mockup<F>
where
    F: Fn() -> Element + 'static,
{
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        (self.view_fn)()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}
