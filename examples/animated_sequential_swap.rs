use std::cell::Cell;
use std::time::Duration;

use tui_lipan::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SwapPhase {
    Idle,
    FadingOut,
}

struct State {
    fg_bg_slot: usize,
    fg_bg_phase: SwapPhase,
    fg_only_slot: usize,
    fg_only_phase: SwapPhase,
    frame_slot: usize,
    frame_phase: SwapPhase,
    target_slot: usize,
    target_phase: SwapPhase,
    crossfade_slot: usize,
    /// The slot the crossfade last started for, so a change restarts it from the old page.
    crossfade_seen: Cell<Option<usize>>,
}

#[derive(Clone, Debug)]
enum Msg {
    NextFgBg,
    NextFgOnly,
    NextFrame,
    NextOpacityTarget,
    NextCrossfade,
    OpacityEndFgBg,
    OpacityEndFgOnly,
    OpacityEndFrame,
    OpacityEndTarget,
    Quit,
}

struct Demo;

const TRANSITION: TransitionConfig = TransitionConfig {
    duration: Duration::from_millis(240),
    easing: Easing::EaseInOutCubic,
};

const INSTANT: TransitionConfig = TransitionConfig {
    duration: Duration::ZERO,
    easing: Easing::Linear,
};

const CROSSFADE: TransitionConfig = TransitionConfig {
    duration: Duration::from_millis(480),
    easing: Easing::EaseInOutCubic,
};

const CROSSFADE_PAGES: &[(Color, Color, &str, &str)] = &[
    (
        Color::Rgb(255, 214, 165),
        Color::Rgb(70, 36, 36),
        "Launcher",
        "Pick a session, or press n to start a new one.",
    ),
    (
        Color::Rgb(165, 243, 252),
        Color::Rgb(18, 52, 64),
        "Session",
        "~/src $ cargo run",
    ),
    (
        Color::Rgb(210, 200, 255),
        Color::Rgb(40, 30, 70),
        "Another session with a longer title",
        "Glyphs dissolve cell by cell while colors blend.",
    ),
];

const FG_BG_PAGES: &[(Color, Color, &str, &str)] = &[
    (
        Color::Rgb(255, 214, 165),
        Color::Rgb(90, 40, 40),
        "Sunrise",
        "Outgoing warm fg + deep panel bg.",
    ),
    (
        Color::Rgb(165, 243, 252),
        Color::Rgb(15, 60, 72),
        "Lagoon",
        "Cool text on teal-tinted backing.",
    ),
    (
        Color::Rgb(210, 200, 255),
        Color::Rgb(45, 32, 78),
        "Lilac",
        "Soft violet copy on a dark purple slab.",
    ),
];

const FG_ONLY_PAGES: &[(Color, &str, &str)] = &[
    (
        Color::Rgb(255, 160, 122),
        "Alerts",
        "Only the wrapper fg animates; subtree uses default inheritance.",
    ),
    (
        Color::Rgb(144, 238, 144),
        "OK",
        "Swap at opacity zero keeps glyph transitions honest.",
    ),
    (
        Color::Rgb(135, 206, 250),
        "Info",
        "Press [2] again to rotate this lane.",
    ),
];

const OPACITY_TARGET_PAGES: &[(&str, &str)] = &[
    (
        "Scarlet wash",
        "Opacity blends toward Color::Red, not the terminal background.",
    ),
    (
        "Sequential swap",
        "At full fade the text is red-tinted; then content swaps and fades back in.",
    ),
    (
        "Fg-only + target",
        "opacity_fg_only keeps the frame fill solid while glyphs wash to red.",
    ),
];

const FRAME_FG_PAGES: &[(Color, &str, &str)] = &[
    (
        Color::Rgb(255, 99, 99),
        "Error tone",
        "Frame bg stays fixed; Animated tweens fg only.",
    ),
    (
        Color::Rgb(80, 250, 123),
        "Success tone",
        "Use this pattern when the panel chrome is stable.",
    ),
    (
        Color::Rgb(255, 184, 108),
        "Warning tone",
        "Press [3] to cycle this row.",
    ),
];

impl Component for Demo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            fg_bg_slot: 0,
            fg_bg_phase: SwapPhase::Idle,
            fg_only_slot: 0,
            fg_only_phase: SwapPhase::Idle,
            frame_slot: 0,
            frame_phase: SwapPhase::Idle,
            target_slot: 0,
            target_phase: SwapPhase::Idle,
            crossfade_slot: 0,
            crossfade_seen: Cell::new(None),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::NextFgBg => {
                if ctx.state.fg_bg_phase == SwapPhase::Idle {
                    ctx.state.fg_bg_phase = SwapPhase::FadingOut;
                }
            }
            Msg::NextFgOnly => {
                if ctx.state.fg_only_phase == SwapPhase::Idle {
                    ctx.state.fg_only_phase = SwapPhase::FadingOut;
                }
            }
            Msg::NextFrame => {
                if ctx.state.frame_phase == SwapPhase::Idle {
                    ctx.state.frame_phase = SwapPhase::FadingOut;
                }
            }
            Msg::NextOpacityTarget => {
                if ctx.state.target_phase == SwapPhase::Idle {
                    ctx.state.target_phase = SwapPhase::FadingOut;
                }
            }
            Msg::NextCrossfade => {
                ctx.state.crossfade_slot = (ctx.state.crossfade_slot + 1) % CROSSFADE_PAGES.len();
            }
            Msg::OpacityEndFgBg => {
                if ctx.state.fg_bg_phase == SwapPhase::FadingOut {
                    ctx.state.fg_bg_slot = (ctx.state.fg_bg_slot + 1) % FG_BG_PAGES.len();
                    ctx.state.fg_bg_phase = SwapPhase::Idle;
                }
            }
            Msg::OpacityEndFgOnly => {
                if ctx.state.fg_only_phase == SwapPhase::FadingOut {
                    ctx.state.fg_only_slot = (ctx.state.fg_only_slot + 1) % FG_ONLY_PAGES.len();
                    ctx.state.fg_only_phase = SwapPhase::Idle;
                }
            }
            Msg::OpacityEndFrame => {
                if ctx.state.frame_phase == SwapPhase::FadingOut {
                    ctx.state.frame_slot = (ctx.state.frame_slot + 1) % FRAME_FG_PAGES.len();
                    ctx.state.frame_phase = SwapPhase::Idle;
                }
            }
            Msg::OpacityEndTarget => {
                if ctx.state.target_phase == SwapPhase::FadingOut {
                    ctx.state.target_slot =
                        (ctx.state.target_slot + 1) % OPACITY_TARGET_PAGES.len();
                    ctx.state.target_phase = SwapPhase::Idle;
                }
            }
            Msg::Quit => ctx.quit(),
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let msg = match key.code {
            KeyCode::Char('1') => Some(Msg::NextFgBg),
            KeyCode::Char('2') => Some(Msg::NextFgOnly),
            KeyCode::Char('3') => Some(Msg::NextFrame),
            KeyCode::Char('4') => Some(Msg::NextOpacityTarget),
            KeyCode::Char('5') => Some(Msg::NextCrossfade),
            KeyCode::Char('q') | KeyCode::Esc => Some(Msg::Quit),
            _ => None,
        };
        match msg {
            Some(m) => {
                self.update(m, ctx);
                KeyUpdate::handled(Update::full())
            }
            None => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let fg_bg_opacity = match ctx.state.fg_bg_phase {
            SwapPhase::Idle => 1.0,
            SwapPhase::FadingOut => 0.0,
        };
        let fg_only_opacity = match ctx.state.fg_only_phase {
            SwapPhase::Idle => 1.0,
            SwapPhase::FadingOut => 0.0,
        };
        let frame_opacity = match ctx.state.frame_phase {
            SwapPhase::Idle => 1.0,
            SwapPhase::FadingOut => 0.0,
        };
        let target_opacity = match ctx.state.target_phase {
            SwapPhase::Idle => 1.0,
            SwapPhase::FadingOut => 0.0,
        };

        let (fg_b, bg_b, title_b, blurb_b) = FG_BG_PAGES[ctx.state.fg_bg_slot];
        let (fg_o, title_o, blurb_o) = FG_ONLY_PAGES[ctx.state.fg_only_slot];
        let (fg_f, title_f, blurb_f) = FRAME_FG_PAGES[ctx.state.frame_slot];
        let (title_t, blurb_t) = OPACITY_TARGET_PAGES[ctx.state.target_slot];

        let fg_bg_block = Animated::new(
            Frame::new()
                .header_left(title_b)
                .border(true)
                .padding(1)
                .child(
                    VStack::new()
                        .gap(1)
                        .child(Text::new("Fg + bg on Animated").style(Style::new().bold()))
                        .child(Text::new(blurb_b))
                        .key(format!("fg-bg-{}", ctx.state.fg_bg_slot)),
                ),
        )
        .fg(fg_b)
        .bg(bg_b)
        .opacity(fg_bg_opacity)
        .transition(TRANSITION)
        .on_opacity_transition_end(ctx.link().callback(|_| Msg::OpacityEndFgBg));

        let fg_only_block = Animated::new(
            VStack::new()
                .gap(1)
                .child(Text::new(title_o).style(Style::new().bold()))
                .child(Text::new(blurb_o))
                .key(format!("fg-only-{}", ctx.state.fg_only_slot)),
        )
        .fg(fg_o)
        .opacity(fg_only_opacity)
        .transition(TRANSITION)
        .on_opacity_transition_end(ctx.link().callback(|_| Msg::OpacityEndFgOnly));

        let frame_block = Frame::new()
            .header_left("Fixed frame background")
            .border(true)
            .padding(1)
            .style(Style::new().bg(Color::Rgb(36, 40, 48)))
            .child(
                Animated::new(
                    VStack::new()
                        .gap(1)
                        .child(Text::new(title_f).style(Style::new().bold()))
                        .child(Text::new(blurb_f))
                        .key(format!("frame-slot-{}", ctx.state.frame_slot)),
                )
                .fg(fg_f)
                .opacity(frame_opacity)
                .opacity_fg_only(true)
                .transition(TRANSITION)
                .on_opacity_transition_end(ctx.link().callback(|_| Msg::OpacityEndFrame)),
            );

        let target_block = Frame::new()
            .header_left("opacity_target(Color::Red)")
            .border(true)
            .padding(1)
            .style(Style::new().bg(Color::Rgb(28, 32, 40)))
            .child(
                Animated::new(
                    VStack::new()
                        .gap(1)
                        .child(Text::new(title_t).style(Style::new().bold()))
                        .child(Text::new(blurb_t))
                        .key(format!("opacity-target-{}", ctx.state.target_slot)),
                )
                .fg(Color::Rgb(220, 230, 240))
                .opacity_target(Color::Red)
                .opacity(target_opacity)
                .opacity_fg_only(true)
                .transition(TRANSITION)
                .on_opacity_transition_end(ctx.link().callback(|_| Msg::OpacityEndTarget)),
            );

        let crossfade_block = crossfade(ctx);

        Frame::new()
            .header_left("Sequential Animated swap")
            .footer_left(
                "[1] fg+bg  [2] fg only  [3] frame+fg  [4] opacity target  [5] crossfade  [q] quit",
            )
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(
                            "[1]-[4]: fade out, swap in on_opacity_transition_end, fade in. [5]: crossfade both at once.",
                        )
                        .style(Style::new().dim()),
                    )
                    .child(fg_bg_block)
                    .child(fg_only_block)
                    .child(frame_block)
                    .child(target_block)
                    .child(crossfade_block),
            )
            .into()
    }
}

/// Both pages on screen at once, rather than one after the other: the outgoing page stays whole
/// beneath while the incoming one fades in over it.
///
/// An `Animated` with an opacity below 1 and no `opacity_target` composites over whatever is
/// painted beneath it. Colors blend, and where the two pages draw different glyphs the stronger
/// one is shown, so the old text fades out as the new text fades in. `auto_exit` keeps the removed
/// page painted, and `keep_opacity` keeps it whole until the new one has covered it.
fn crossfade(ctx: &Context<Demo>) -> Element {
    const KEY: &str = "crossfade-reveal";
    let slot = ctx.state.crossfade_slot;
    // A new page starts transparent: a newly mounted `Animated` takes the opacity it is given at
    // once, so the fade is driven here and handed to it every frame. The first page just appears.
    if ctx
        .state
        .crossfade_seen
        .replace(Some(slot))
        .is_some_and(|seen| seen != slot)
    {
        ctx.transition(KEY, 0.0, INSTANT);
    }
    let opacity = ctx.transition(KEY, 1.0, CROSSFADE);
    let (fg, bg, title, body) = CROSSFADE_PAGES[slot];
    let page = Frame::new()
        .header_left(title)
        .border(true)
        // Each page covers the last: replace its title rather than keep it as a neighbour's.
        .border_merge_mode(BorderMergeMode::Replace)
        .style(Style::new().bg(bg))
        .child(Text::new(body).style(Style::new().fg(fg)));
    // Alone in its stack, so the retained outgoing page is drawn beneath the live one.
    ZStack::new()
        .child(
            Animated::new(page)
                .opacity(opacity)
                .transition(INSTANT)
                .auto_exit(ExitAnimation::new(CROSSFADE.duration.as_millis() as u64).keep_opacity())
                .key(format!("crossfade-{slot}")),
        )
        .min_height(Length::Px(3))
        .max_height(Length::Px(3))
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Sequential Animated swap")
        .terminal_bg(query_host_colors().map(|c| c.bg))
        .mount(Demo)
        .run()
}
