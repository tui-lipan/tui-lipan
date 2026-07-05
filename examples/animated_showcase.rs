use std::time::Duration;

use tui_lipan::prelude::*;

struct AnimatedShowcase;

const EASINGS: [Easing; 6] = [
    Easing::Linear,
    Easing::EaseInQuad,
    Easing::EaseOutQuad,
    Easing::EaseInOutCubic,
    Easing::EaseInOutSine,
    Easing::EaseOutElastic,
];

const DURATIONS_MS: [u64; 4] = [120, 220, 360, 650];

struct State {
    fade_visible: bool,
    reveal_open: bool,
    reveal_height_layout_idle: bool,
    combo_open: bool,
    combo_height_layout_idle: bool,
    pulse_open: bool,
    pulse_height_layout_idle: bool,
    position_at_end: bool,
    auto_pulse: bool,
    easing_idx: usize,
    duration_idx: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    ToggleFade,
    ToggleReveal,
    ToggleCombo,
    TogglePulse,
    TogglePosition,
    ToggleAutoPulse,
    CycleEasing,
    CycleDuration,
    Tick,
    RevealHeightSettled,
    ComboHeightSettled,
    PulseHeightSettled,
}

impl Component for AnimatedShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            fade_visible: true,
            reveal_open: true,
            reveal_height_layout_idle: true,
            combo_open: true,
            combo_height_layout_idle: true,
            pulse_open: false,
            pulse_height_layout_idle: true,
            position_at_end: false,
            auto_pulse: true,
            easing_idx: 3,
            duration_idx: 1,
        }
    }

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        Some(schedule_tick())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ToggleFade => ctx.state.fade_visible = !ctx.state.fade_visible,
            Msg::ToggleReveal => {
                ctx.state.reveal_open = !ctx.state.reveal_open;
                ctx.state.reveal_height_layout_idle = false;
            }
            Msg::ToggleCombo => {
                ctx.state.combo_open = !ctx.state.combo_open;
                ctx.state.combo_height_layout_idle = false;
            }
            Msg::TogglePulse => {
                ctx.state.pulse_open = !ctx.state.pulse_open;
                ctx.state.pulse_height_layout_idle = false;
            }
            Msg::TogglePosition => ctx.state.position_at_end = !ctx.state.position_at_end,
            Msg::ToggleAutoPulse => ctx.state.auto_pulse = !ctx.state.auto_pulse,
            Msg::CycleEasing => {
                ctx.state.easing_idx = (ctx.state.easing_idx + 1) % EASINGS.len();
            }
            Msg::CycleDuration => {
                ctx.state.duration_idx = (ctx.state.duration_idx + 1) % DURATIONS_MS.len();
            }
            Msg::Tick => {
                if ctx.state.auto_pulse {
                    ctx.state.pulse_open = !ctx.state.pulse_open;
                    ctx.state.pulse_height_layout_idle = false;
                }
                return Update::with_command(schedule_tick());
            }
            Msg::RevealHeightSettled => ctx.state.reveal_height_layout_idle = true,
            Msg::ComboHeightSettled => ctx.state.combo_height_layout_idle = true,
            Msg::PulseHeightSettled => ctx.state.pulse_height_layout_idle = true,
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let update = match key.code {
            KeyCode::Char('1') => Some(Msg::ToggleFade),
            KeyCode::Char('2') => Some(Msg::ToggleReveal),
            KeyCode::Char('3') => Some(Msg::ToggleCombo),
            KeyCode::Char('4') => Some(Msg::TogglePulse),
            KeyCode::Char('5') => Some(Msg::TogglePosition),
            KeyCode::Char('a') => Some(Msg::ToggleAutoPulse),
            KeyCode::Char('d') => Some(Msg::CycleDuration),
            KeyCode::Char('e') => Some(Msg::CycleEasing),
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                return KeyUpdate::handled(Update::full());
            }
            _ => return KeyUpdate::unhandled(Update::none()),
        };

        self.update(update.expect("key match should produce message"), ctx);
        KeyUpdate::handled(Update::full())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let small = matches!(ctx.breakpoint(120, 180), Breakpoint::Small);
        let transition = current_transition(&ctx.state);

        let left = VStack::new()
            .gap(1)
            .width(Length::Flex(1))
            .child(self.fade_panel(ctx, transition))
            .child(self.height_panel(ctx, transition))
            .child(self.position_panel(ctx, transition));

        let right = VStack::new()
            .gap(1)
            .width(Length::Flex(1))
            .child(self.combo_panel(ctx, transition))
            .child(self.pulse_panel(ctx, transition));

        let body: Element = if small {
            VStack::new().gap(1).child(left).child(right).into()
        } else {
            HStack::new().gap(1).child(left).child(right).into()
        };

        Frame::new()
            .title("Animated Showcase")
            .status(format!(
                "{} • {}ms • 1/2/3/4/5 toggle • e/d tune • a autoplay • q quit",
                easing_name(EASINGS[ctx.state.easing_idx]),
                DURATIONS_MS[ctx.state.duration_idx]
            ))
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "Animated wraps any subtree and interpolates opacity, revealed height, and keyed position changes.",
                        )
                        .style(Style::new().bold().fg(Color::Rgb(132, 201, 255))),
                    )
                    .child(
                        Text::new(
                            "Use the buttons or hotkeys to compare fades, height reveals, FLIP movement, combined transitions, and a looping pulse.",
                        )
                        .style(Style::new().dim()),
                    )
                    .child(self.controls(ctx))
                    .child(body),
            )
            .into()
    }
}

impl AnimatedShowcase {
    fn controls(&self, ctx: &Context<Self>) -> Element {
        let primary = HStack::new()
            .gap(1)
            .height(Length::Auto)
            .child(
                Button::new(if ctx.state.fade_visible {
                    "Hide Fade [1]"
                } else {
                    "Show Fade [1]"
                })
                .on_click(ctx.link().callback(|_| Msg::ToggleFade)),
            )
            .child(
                Button::new(if ctx.state.reveal_open {
                    "Collapse Height [2]"
                } else {
                    "Expand Height [2]"
                })
                .on_click(ctx.link().callback(|_| Msg::ToggleReveal)),
            )
            .child(
                Button::new(if ctx.state.combo_open {
                    "Hide Combo [3]"
                } else {
                    "Show Combo [3]"
                })
                .on_click(ctx.link().callback(|_| Msg::ToggleCombo)),
            )
            .child(
                Button::new(if ctx.state.pulse_open {
                    "Toggle Pulse Off [4]"
                } else {
                    "Toggle Pulse On [4]"
                })
                .on_click(ctx.link().callback(|_| Msg::TogglePulse)),
            )
            .child(
                Button::new(if ctx.state.position_at_end {
                    "Move Card Up [5]"
                } else {
                    "Move Card Down [5]"
                })
                .on_click(ctx.link().callback(|_| Msg::TogglePosition)),
            );

        let secondary = HStack::new()
            .gap(1)
            .height(Length::Auto)
            .child(
                Button::new(format!(
                    "Easing: {} [e]",
                    easing_name(EASINGS[ctx.state.easing_idx])
                ))
                .on_click(ctx.link().callback(|_| Msg::CycleEasing)),
            )
            .child(
                Button::new(format!(
                    "Duration: {}ms [d]",
                    DURATIONS_MS[ctx.state.duration_idx]
                ))
                .on_click(ctx.link().callback(|_| Msg::CycleDuration)),
            )
            .child(
                Button::new(if ctx.state.auto_pulse {
                    "Autoplay: On [a]"
                } else {
                    "Autoplay: Off [a]"
                })
                .on_click(ctx.link().callback(|_| Msg::ToggleAutoPulse)),
            );

        VStack::new()
            .gap(1)
            .height(Length::Auto)
            .child(primary)
            .child(secondary)
            .into()
    }

    fn fade_panel(&self, ctx: &Context<Self>, transition: TransitionConfig) -> Element {
        Frame::new()
            .title("Opacity Fade")
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Opacity changes without collapsing the slot."))
                    .child(
                        Animated::new(
                            Frame::new()
                                .title("Reserved Layout")
                                .border(true)
                                .padding(1)
                                .style(Style::new().bg(Color::indexed(236)))
                                .child(
                                    VStack::new()
                                        .gap(1)
                                        .child(
                                            Text::new("Fade target: 1.0 <-> 0.0")
                                                .style(Style::new().bold()),
                                        )
                                        .child(Text::new(
                                            "The frame stays in layout even when it becomes fully transparent.",
                                        ))
                                        .child(
                                            Text::new(if ctx.state.fade_visible {
                                                "Currently visible"
                                            } else {
                                                "Currently transparent"
                                            })
                                            .style(Style::new().fg(Color::Rgb(255, 205, 112))),
                                        ),
                                ),
                        )
                        .opacity(if ctx.state.fade_visible { 1.0 } else { 0.0 })
                        .transition(transition),
                    ),
            )
            .into()
    }

    fn height_panel(&self, ctx: &Context<Self>, transition: TransitionConfig) -> Element {
        let closing = !ctx.state.reveal_open && !ctx.state.reveal_height_layout_idle;
        Frame::new()
            .title("Height Reveal")
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(
                        "Height animates between Length::Px(0) and Length::Auto.",
                    ))
                    .child(
                        Animated::new(
                            Frame::new()
                                .title("Natural Content Height")
                                .border(true)
                                .padding(1)
                                .style(Style::new().bg(Color::indexed(235)))
                                .child(
                                    VStack::new()
                                        .gap(1)
                                        .child(Text::new(
                                            "This section uses only height animation.",
                                        ))
                                        .child(Text::new("Line 1: measured from child content."))
                                        .child(Text::new("Line 2: clamped during the transition."))
                                        .child(Text::new("Line 3: useful for reveal/collapse UI.")),
                                ),
                        )
                        .height(if ctx.state.reveal_open {
                            Length::Auto
                        } else {
                            Length::Px(0)
                        })
                        .layout_height(if closing { Some(Length::Auto) } else { None })
                        .on_height_transition_end(ctx.link().callback(|_| Msg::RevealHeightSettled))
                        .transition(transition),
                    ),
            )
            .into()
    }

    fn position_panel(&self, ctx: &Context<Self>, transition: TransitionConfig) -> Element {
        let card: Element = Animated::new(
            Frame::new()
                .title("Keyed Card")
                .border(true)
                .padding(1)
                .style(Style::new().bg(Color::indexed(236)))
                .child(
                    Text::new("Same .key(...), new stack slot")
                        .style(Style::new().bold().fg(Color::Rgb(169, 255, 214))),
                ),
        )
        .position_transition(true)
        .transition(transition)
        .into();
        let card = card.key("animated-position-card");

        let rail: Element = if ctx.state.position_at_end {
            VStack::new()
                .height(Length::Px(10))
                .child(Text::new("origin slot"))
                .child(Spacer::new())
                .child(card)
                .into()
        } else {
            VStack::new()
                .height(Length::Px(10))
                .child(card)
                .child(Spacer::new())
                .child(Text::new("destination slot"))
                .into()
        };

        Frame::new()
            .title("Position Transition")
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(
                        "A keyed Animated wrapper moves visually while layout snaps first.",
                    ))
                    .child(rail),
            )
            .into()
    }

    fn combo_panel(&self, ctx: &Context<Self>, transition: TransitionConfig) -> Element {
        let closing = !ctx.state.combo_open && !ctx.state.combo_height_layout_idle;
        Frame::new()
            .title("Fade + Expand")
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(
                        "The common enter/exit pattern uses both channels together.",
                    ))
                    .child(
                        Animated::new(
                            Frame::new()
                                .title("Deploy Summary")
                                .border(true)
                                .padding(1)
                                .style(Style::new().bg(Color::indexed(236)))
                                .child(
                                    VStack::new()
                                        .gap(1)
                                        .child(
                                            Text::new("Release candidate ready")
                                                .style(Style::new().bold().fg(Color::LightGreen)),
                                        )
                                        .child(Text::new("- 18 checks passed"))
                                        .child(Text::new("- 3 environments verified"))
                                        .child(Text::new("- rollout can begin after approval")),
                                ),
                        )
                        .opacity(if ctx.state.combo_open { 1.0 } else { 0.0 })
                        .height(if ctx.state.combo_open {
                            Length::Auto
                        } else {
                            Length::Px(0)
                        })
                        .layout_height(if closing { Some(Length::Auto) } else { None })
                        .on_height_transition_end(ctx.link().callback(|_| Msg::ComboHeightSettled))
                        .transition(transition),
                    ),
            )
            .into()
    }

    fn pulse_panel(&self, ctx: &Context<Self>, transition: TransitionConfig) -> Element {
        let closing = !ctx.state.pulse_open && !ctx.state.pulse_height_layout_idle;
        Frame::new()
            .title("Looping Pulse")
            .border(true)
            .padding(1)
            .width(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(if ctx.state.auto_pulse {
                        "Autoplay flips the target every 800ms. Try EaseOutElastic for visible overshoot."
                    } else {
                        "Autoplay is off. Use [4] or the button row to trigger this panel manually."
                    }))
                    .child(
                        Animated::new(
                            Frame::new()
                                .title("Live Heartbeat")
                                .border(true)
                                .padding(1)
                                .style(Style::new().bg(Color::indexed(234)))
                                .child(
                                    VStack::new()
                                        .gap(1)
                                        .child(Text::new("worker-a    42 req/s"))
                                        .child(Text::new("worker-b    37 req/s"))
                                        .child(Text::new("cache hit   96.2%"))
                                        .child(Text::new("queue lag   12ms")),
                                ),
                        )
                        .opacity(if ctx.state.pulse_open { 1.0 } else { 0.35 })
                        .height(if ctx.state.pulse_open {
                            Length::Auto
                        } else {
                            Length::Px(2)
                        })
                        .layout_height(if closing { Some(Length::Auto) } else { None })
                        .on_height_transition_end(ctx.link().callback(|_| Msg::PulseHeightSettled))
                        .transition(transition),
                    ),
            )
            .into()
    }
}

fn current_transition(state: &State) -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(DURATIONS_MS[state.duration_idx]),
        easing: EASINGS[state.easing_idx],
    }
}

fn easing_name(easing: Easing) -> &'static str {
    match easing {
        Easing::Linear => "Linear",
        Easing::EaseInQuad => "EaseInQuad",
        Easing::EaseOutQuad => "EaseOutQuad",
        Easing::EaseInOutCubic => "EaseInOutCubic",
        Easing::EaseInOutSine => "EaseInOutSine",
        Easing::EaseOutElastic => "EaseOutElastic",
    }
}

fn schedule_tick() -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(Duration::from_millis(800));
        link.send(Msg::Tick);
    })
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Animated Showcase")
        .terminal_bg(query_host_colors().map(|c| c.bg))
        .mount(AnimatedShowcase)
        .run()
}
