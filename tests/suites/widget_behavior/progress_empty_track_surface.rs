//! The empty half of a `ProgressStyle::Block` track must recede toward the
//! surface behind the bar, not toward black.
//!
//! Dimming the fill color (the old behavior) is invisible on a dark theme but
//! paints a black trough across a light one, which is what this guards.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedCell, TestBackend};

#[derive(Clone, Copy)]
struct Bar {
    theme: fn() -> Theme,
}

impl Component for Bar {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ThemeProvider::new((self.theme)())
            .child(ProgressBar::new(0.5).width(Length::Px(10)))
            .into()
    }
}

/// Render the bar and return the last track cell, which is always empty at 50%.
fn last_track_cell(theme: fn() -> Theme) -> CapturedCell {
    let mut backend = TestBackend::new(Bar { theme });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    });
    backend.render();
    backend.capture_frame().cell(9, 0).clone()
}

#[test]
fn empty_track_stays_light_on_a_light_theme() {
    for (name, theme) in [
        ("solarized_light", Theme::solarized_light as fn() -> Theme),
        ("catppuccin_latte", Theme::catppuccin_latte),
        ("rose_pine_dawn", Theme::rose_pine_dawn),
        ("gruvbox_light", Theme::gruvbox_light),
        ("ayu_light", Theme::ayu_light),
        ("tokyo_night_day", Theme::tokyo_night_day),
    ] {
        let bg = last_track_cell(theme).bg;
        assert!(
            bg.luminance() > 0.5,
            "{name}: empty track background {bg:?} is dark on a light theme"
        );
    }
}

#[test]
fn empty_track_stays_near_the_backdrop_on_a_dark_theme() {
    for (name, theme) in [
        ("lipan", Theme::lipan as fn() -> Theme),
        ("dracula", Theme::dracula),
        ("kanagawa", Theme::kanagawa),
        ("zenburn", Theme::zenburn),
    ] {
        let theme_bg = theme()
            .primary
            .bg
            .map(|p| p.color())
            .expect("preset sets a background");
        let track_bg = last_track_cell(theme).bg;
        let delta = (track_bg.luminance() - theme_bg.luminance()).abs();
        assert!(
            delta < 0.2,
            "{name}: empty track {track_bg:?} strays too far from backdrop {theme_bg:?}"
        );
    }
}

#[test]
fn empty_track_remains_distinguishable_from_the_filled_half() {
    // Receding toward the surface must not make the track invisible: the empty
    // and filled halves still have to read as different.
    for (name, theme) in [
        ("solarized_light", Theme::solarized_light as fn() -> Theme),
        ("lipan", Theme::lipan),
        ("kanagawa", Theme::kanagawa),
    ] {
        let mut backend = TestBackend::new(Bar { theme });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 1,
        });
        backend.render();
        let frame = backend.capture_frame();
        let filled = frame.cell(0, 0).bg;
        let empty = frame.cell(9, 0).bg;
        assert_ne!(filled, empty, "{name}: track halves are indistinguishable");
    }
}
