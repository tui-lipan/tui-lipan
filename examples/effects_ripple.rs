use std::time::Duration;

use tui_lipan::prelude::*;

const BURST_DURATION_TICKS: u32 = 44;

struct RippleEffectsDemo;

#[derive(Default)]
struct State {
    burst_start: Option<u64>,
    burst_generation: u64,
}

#[derive(Clone, Debug)]
enum Msg {
    Burst,
    ClearBurst(u64),
}

impl Component for RippleEffectsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Burst => {
                ctx.state.burst_generation = ctx.state.burst_generation.wrapping_add(1);
                ctx.state.burst_start = Some(ctx.effect_phase());
                let generation = ctx.state.burst_generation;
                Update::with_command(ctx.link().command_keyed(
                    "effects-ripple-clear-burst",
                    TaskPolicy::LatestOnly,
                    move |link| {
                        std::thread::sleep(Duration::from_millis(BURST_DURATION_TICKS as u64 * 17));
                        link.send(Msg::ClearBurst(generation));
                    },
                ))
            }
            Msg::ClearBurst(generation) if generation == ctx.state.burst_generation => {
                ctx.state.burst_start = None;
                Update::full()
            }
            Msg::ClearBurst(_) => Update::none(),
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('b') | KeyCode::Enter => {
                ctx.link().send(Msg::Burst);
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
        let mut effects = vec![VisualEffect::centered_looping_ripple(
            24.0,
            96,
            1.4,
            Color::rgb(80, 190, 255),
            0.42,
        )];

        if let Some(start_tick) = ctx.state.burst_start {
            effects.push(VisualEffect::centered_burst_ripple(
                30.0,
                BURST_DURATION_TICKS,
                start_tick,
                1.8,
                Color::rgb(255, 220, 110),
                0.8,
            ));
        }

        let shell = Frame::new()
            .title("Ripple Effects")
            .status("b/enter burst | q quit")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "Loop is phase-driven by the renderer; Burst captures ctx.effect_phase().",
                        )
                        .style(Style::new().fg(Color::rgb(155, 210, 255)).bold()),
                    )
                    .child(ripple_panel(ctx))
                    .child(
                        Button::new("trigger burst")
                            .on_click(ctx.link().callback(|_| Msg::Burst))
                            .style(Style::new().fg(Color::rgb(255, 220, 110))),
                    ),
            );

        EffectScope::new().effects(effects).child(shell).into()
    }
}

fn ripple_panel(ctx: &Context<RippleEffectsDemo>) -> Element {
    let burst = ctx.state.burst_start.map_or("idle".to_string(), |tick| {
        format!("burst start tick: {tick}")
    });

    Frame::new()
        .title("Loop + Once")
        .border(true)
        .height(Length::Flex(1))
        .style(Style::new().bg(Color::rgb(8, 12, 22)))
        .child(
            ZStack::new()
                .style(Style::new().bg(Color::rgb(8, 12, 22)))
                .child(background_grid())
                .child(
                    Center::new().child(
                        VStack::new()
                            .gap(1)
                            .align(Align::Center)
                            .child(
                                Text::new("renderer-owned ripple")
                                    .style(Style::new().fg(Color::rgb(240, 248, 255)).bold()),
                            )
                            .child(
                                Text::new(burst).style(Style::new().fg(Color::rgb(170, 190, 215))),
                            ),
                    ),
                ),
        )
        .into()
}

fn background_grid() -> Element {
    let style = Style::new().fg(Color::rgb(70, 92, 128));
    VStack::new()
        .child(Text::new("..::..::..::..::..::..::..::..::..::..::..").style(style))
        .child(Text::new("==++==++==++==++==++==++==++==++==++==++==").style(style))
        .child(Text::new("<><><><><><><><><><><><><><><><><><><><><>").style(style))
        .child(Text::new("..::..::..::..::..::..::..::..::..::..::..").style(style))
        .child(Text::new("==++==++==++==++==++==++==++==++==++==++==").style(style))
        .child(Text::new("<><><><><><><><><><><><><><><><><><><><><>").style(style))
        .child(Text::new("..::..::..::..::..::..::..::..::..::..::..").style(style))
        .into()
}

fn main() -> Result<()> {
    App::new()
        .title("Ripple Effects")
        .mount(RippleEffectsDemo)
        .run()
}
