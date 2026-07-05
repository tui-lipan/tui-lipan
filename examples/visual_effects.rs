use tui_lipan::prelude::*;
use tui_lipan::{EffectAxis, EffectPalette};

const RETRO_PRESETS: [RetroPreset; 5] = [
    RetroPreset::Amber,
    RetroPreset::Green,
    RetroPreset::Cga,
    RetroPreset::Gameboy,
    RetroPreset::VaultTec,
];

const PALETTES: [EffectPalette; 4] = [
    EffectPalette::Amber,
    EffectPalette::Green,
    EffectPalette::Cga,
    EffectPalette::Gameboy,
];

struct VisualEffectsDemo;

struct State {
    retro_idx: usize,
    palette_idx: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    NextRetro,
    NextPalette,
}

impl Component for VisualEffectsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            retro_idx: RETRO_PRESETS.len() - 1,
            palette_idx: 0,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::NextRetro => {
                ctx.state.retro_idx = (ctx.state.retro_idx + 1) % RETRO_PRESETS.len();
            }
            Msg::NextPalette => {
                ctx.state.palette_idx = (ctx.state.palette_idx + 1) % PALETTES.len();
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let msg = match key.code {
            KeyCode::Char('1') => Some(Msg::NextRetro),
            KeyCode::Char('2') => Some(Msg::NextPalette),
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                return KeyUpdate::handled(Update::full());
            }
            _ => None,
        };

        if let Some(msg) = msg {
            ctx.link().send(msg);
            KeyUpdate::handled(Update::full())
        } else {
            KeyUpdate::unhandled(Update::none())
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let retro = RETRO_PRESETS[ctx.state.retro_idx];
        let palette = PALETTES[ctx.state.palette_idx].clone();
        let palette_label = format!("{:?}", palette);
        let small = matches!(ctx.breakpoint(110, 160), Breakpoint::Small);

        let top_row: Element = if small {
            VStack::new()
                .gap(1)
                .height(Length::Flex(1))
                .child(static_palette_panel(palette))
                .child(animated_wave_panel())
                .into()
        } else {
            HStack::new()
                .gap(1)
                .height(Length::Flex(1))
                .child(static_palette_panel(palette))
                .child(animated_wave_panel())
                .into()
        };

        let body: Element = VStack::new()
            .gap(1)
            .child(top_row)
            .child(retro_zstack_panel(retro))
            .into();

        Frame::new()
            .title("Visual Effects")
            .status("1 retro preset • 2 palette • q quit")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "EffectScope post-processes the final composed subtree, including nested ZStacks.",
                        )
                        .style(Style::new().bold().fg(Color::rgb(140, 208, 255))),
                    )
                    .child(
                        Text::new(format!(
                            "Retro preset: {:?} | Palette: {:?} | Rainbow wave animates without an explicit component tick.",
                            retro, palette_label
                        ))
                        .style(Style::new().dim()),
                    )
                    .child(body),
            )
            .into()
    }
}

fn static_palette_panel(palette: EffectPalette) -> Element {
    Frame::new()
        .title("Palette + Scanlines")
        .border(true)
        .height(Length::Flex(1))
        .child(
            EffectScope::new()
                .effect(VisualEffect::PaletteQuantize { palette })
                .effect(VisualEffect::Scanlines {
                    strength: 0.18,
                    spacing: 2,
                })
                .child(
                    VStack::new()
                        .style(Style::new().bg(Color::rgb(8, 12, 20)))
                        .child(sample_row("CPU", Color::rgb(255, 92, 87), 0.83))
                        .child(sample_row("MEM", Color::rgb(255, 189, 46), 0.61))
                        .child(sample_row("NET", Color::rgb(39, 201, 63), 0.42))
                        .child(sample_row("IO ", Color::rgb(92, 166, 255), 0.27)),
                ),
        )
        .into()
}

fn animated_wave_panel() -> Element {
    Frame::new()
        .title("Rainbow Wave")
        .border(true)
        .height(Length::Flex(1))
        .child(
            EffectScope::new()
                .effect(VisualEffect::Monochrome { strength: 0.45 })
                .effect(VisualEffect::RainbowWave {
                    blend: 1.0,
                    frequency: 1.35,
                    speed: 1.0,
                    axis: EffectAxis::Diagonal,
                })
                .child(Center::new().child(
                    Text::new("TUI-LIPAN").style(Style::new().bold().fg(Color::rgb(220, 220, 220))),
                )),
        )
        .into()
}

fn retro_zstack_panel(retro: RetroPreset) -> Element {
    Frame::new()
        .title("Retro CRT over ZStack")
        .border(true)
        .height(Length::Flex(1))
        .child(
            EffectScope::new()
                .effect(VisualEffect::RetroCrt {
                    preset: retro,
                    flicker: 0.6,
                    scanline_strength: 0.22,
                })
                .child(
                    ZStack::new()
                        .style(Style::new().bg(Color::rgb(10, 12, 16)))
                        .child(background_grid())
                        .child(
                            Center::new().child(
                                Frame::new()
                                    .title("COMPOSED FIRST")
                                    .border(true)
                                    .width(Length::Px(28))
                                    .height(Length::Px(7))
                                    .style(Style::new().bg(Color::rgb(20, 28, 36)))
                                    .child(
                                        VStack::new()
                                            .gap(1)
                                            .padding(1)
                                            .child(
                                                Text::new("Effects apply after stacking.")
                                                    .style(Style::new().fg(Color::rgb(255, 240, 180))),
                                            )
                                            .child(
                                                Text::new("The frame, text, and backdrop are remapped as one subtree.")
                                                    .style(Style::new().fg(Color::rgb(180, 220, 255))),
                                            ),
                                    ),
                            ),
                        ),
                ),
        )
        .into()
}

fn background_grid() -> Element {
    VStack::new()
        .child(
            Text::new("..::..::..::..::..::..::..::..")
                .style(Style::new().fg(Color::rgb(70, 90, 110))),
        )
        .child(
            Text::new("==++==++==++==++==++==++==++==")
                .style(Style::new().fg(Color::rgb(90, 120, 150))),
        )
        .child(
            Text::new("<>[]<>[]<>[]<>[]<>[]<>[]<>[]<>")
                .style(Style::new().fg(Color::rgb(70, 150, 120))),
        )
        .child(
            Text::new("..::..::..::..::..::..::..::..")
                .style(Style::new().fg(Color::rgb(70, 90, 110))),
        )
        .child(
            Text::new("==++==++==++==++==++==++==++==")
                .style(Style::new().fg(Color::rgb(90, 120, 150))),
        )
        .child(
            Text::new("<>[]<>[]<>[]<>[]<>[]<>[]<>[]<>")
                .style(Style::new().fg(Color::rgb(70, 150, 120))),
        )
        .into()
}

fn sample_row(label: &str, color: Color, value: f64) -> Element {
    HStack::new()
        .gap(1)
        .child(Text::new(label).style(Style::new().fg(color).bold()))
        .child(ProgressBar::new(value).style(Style::new().fg(color)))
        .into()
}

fn main() -> Result<()> {
    App::new()
        .title("Visual Effects")
        .mount(VisualEffectsDemo)
        .run()
}
