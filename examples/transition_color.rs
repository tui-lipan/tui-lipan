//! Property-scoped value transitions via `ctx.transition(...)`.
//!
//! Press Space to toggle. Notice how the colors and the displayed scalar smoothly
//! interpolate between their two target values without wrapping anything in
//! `Animated` or `ZStack`. The right-hand panel shows the same toggle without a
//! transition for comparison.
//!
//! Run with: cargo run --example transition_color

use std::time::Duration;

use tui_lipan::prelude::*;

struct TransitionColorDemo;

#[derive(Default)]
struct State {
    active: bool,
}

const RESTING_BG: Color = Color::Rgb(40, 44, 52);
const ACTIVE_BG: Color = Color::Rgb(70, 130, 200);
const RESTING_FG: Color = Color::Rgb(150, 160, 170);
const ACTIVE_FG: Color = Color::Rgb(255, 255, 255);

impl Component for TransitionColorDemo {
    type Message = ();
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                ctx.state.active = !ctx.state.active;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let active = ctx.state.active;

        // Slow, snappy transition for the colors.
        let color_cfg = TransitionConfig {
            duration: Duration::from_millis(400),
            easing: Easing::EaseInOutCubic,
        };
        // A faster scalar transition for the progress / counter readout.
        let scalar_cfg = TransitionConfig {
            duration: Duration::from_millis(600),
            easing: Easing::EaseOutQuad,
        };

        // Property-scoped transitions: target values flip with state, but the
        // values returned this frame are smoothly interpolated.
        let bg = ctx.transition(
            "box-bg",
            if active { ACTIVE_BG } else { RESTING_BG },
            color_cfg,
        );
        let fg = ctx.transition(
            "box-fg",
            if active { ACTIVE_FG } else { RESTING_FG },
            color_cfg,
        );
        let progress =
            ctx.transition::<f32>("box-progress", if active { 1.0 } else { 0.0 }, scalar_cfg);

        let animated_box = Frame::new()
            .title("With transition")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(Style::new().bg(bg).fg(fg))
            .padding(2)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Press SPACE to toggle"))
                    .child(Text::new(format!(
                        "scalar = {:.2}  (interpolates 0.00 \u{2194} 1.00)",
                        progress
                    )))
                    .child(progress_bar(progress, fg)),
            );

        let plain_box = Frame::new()
            .title("Without transition (snap)")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(
                Style::new()
                    .bg(if active { ACTIVE_BG } else { RESTING_BG })
                    .fg(if active { ACTIVE_FG } else { RESTING_FG }),
            )
            .padding(2)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Compare: the same toggle, no smoothing"))
                    .child(Text::new(format!(
                        "scalar = {:.2}",
                        if active { 1.0_f32 } else { 0.0_f32 }
                    )))
                    .child(progress_bar(
                        if active { 1.0 } else { 0.0 },
                        if active { ACTIVE_FG } else { RESTING_FG },
                    )),
            );

        Frame::new()
            .title("ctx.transition(...) — property-scoped animation")
            .status("SPACE toggle • Q quit")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(HStack::new().gap(2).child(animated_box).child(plain_box))
            .into()
    }
}

fn progress_bar(value: f32, fg: Color) -> Element {
    let value = value.clamp(0.0, 1.0) as f64;
    ProgressBar::new(value)
        .progress_style(ProgressStyle::Block)
        .filled_style(Style::new().fg(fg))
        .empty_style(Style::new().fg(Color::indexed(238)))
        .height(Length::Px(1))
        .into()
}

fn main() -> Result<()> {
    App::new().mount(TransitionColorDemo).run()
}
