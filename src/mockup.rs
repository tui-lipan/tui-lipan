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
//!             .header_left("Panel")
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
/// Press `Esc`, `q`, or the framework quit keys (`Ctrl+Q`, or `Ctrl+C` outside a focused text
/// editor) to quit.
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
///                 .header_left("Hello")
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
///             .header_left("Hello")
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
        if !key.mods.ctrl
            && !key.mods.alt
            && !key.mods.super_key
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::core::event::KeyMods;
    use crate::test_backend::TestBackend;
    use crate::widgets::Text;

    fn mockup_backend(app: App) -> TestBackend<Mockup<impl Fn() -> Element + 'static>> {
        let mut backend =
            TestBackend::new_with_app(app, Mockup::new(|| Text::new("hi").into()), ());
        backend.render();
        backend
    }

    #[test]
    fn mockup_quits_on_esc_q_and_framework_quit_keys() {
        let quit_keys = [
            KeyEvent {
                code: KeyCode::Esc,
                mods: KeyMods::NONE,
            },
            KeyEvent {
                code: KeyCode::Char('q'),
                mods: KeyMods::NONE,
            },
            KeyEvent {
                code: KeyCode::Char('q'),
                mods: KeyMods::CTRL,
            },
            KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods::CTRL,
            },
        ];

        for key in quit_keys {
            let mut backend = mockup_backend(App::new());
            backend.send_key(key).expect("send key");
            assert!(
                backend.core.ctx.should_quit(),
                "mockup should quit on {key:?}"
            );
        }
    }

    #[test]
    fn mockup_respects_disabled_framework_quit_while_keeping_esc_and_q() {
        let mut backend = mockup_backend(App::new().global_quit(None));
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char('q'),
                mods: KeyMods::CTRL,
            })
            .expect("send ctrl-q");
        assert!(!backend.core.ctx.should_quit());

        backend
            .send_key(KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods::CTRL,
            })
            .expect("send ctrl-c");
        assert!(!backend.core.ctx.should_quit());

        backend
            .send_key(KeyEvent {
                code: KeyCode::Char('q'),
                mods: KeyMods::NONE,
            })
            .expect("send q");
        assert!(backend.core.ctx.should_quit());
    }
}
