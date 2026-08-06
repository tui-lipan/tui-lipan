//! Template sketch: a sign-in form.
//!
//! Copy this file to start a new sketch. The shape is the whole pattern - a plain
//! view function with no state, and a `sketch()` that names it and picks
//! viewports.

use tui_lipan::prelude::*;
use tui_lipan::{Result, Sketch};

/// The composition under design. No state, no messages - only layout and style.
fn view() -> Element {
    Frame::new()
        .header_left("Sign In")
        .border(true)
        .child(
            VStack::new()
                .gap(1)
                .padding(1)
                .child(Text::new("Welcome back."))
                .child(
                    Input::new("alice@example.com")
                        .placeholder("Email")
                        .key("email"),
                )
                .child(
                    Input::new("")
                        .mask(Some('*'))
                        .placeholder("Password")
                        .key("password"),
                )
                .child(
                    HStack::new()
                        .gap(2)
                        .child(Button::new("Cancel"))
                        .child(Button::new("Log In")),
                ),
        )
        .into()
}

/// Render the sketch at a realistic viewport plus a roomy fit-to-content pass.
///
/// The fit pass is what exposes flex distribution - buttons drifting away from
/// the form they belong to, a panel growing past its intended width.
///
/// Add `.baseline("tests/ui-baselines")` once the layout settles, and the sketch
/// starts failing when the picture drifts instead of only showing you a picture.
pub fn sketch() -> Result<()> {
    Sketch::view("login", view)
        .viewport(80, 24)
        .fit(20, 8)
        .focus_next(1)
        .write()?;
    Ok(())
}
