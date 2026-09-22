//! Screen transitions built on effects that read their backdrop.
//!
//! The outgoing screen stays beneath the incoming one in a `ZStack`. The incoming screen sits in
//! an `EffectScope` whose custom effect reports `uses_backdrop()`, so every cell can choose between
//! the new screen, the old one beneath it, or a blend of the two.
//!
//! `space` switches screens, `t` cycles the transition, `q` quits.

use std::time::Duration;

use tui_lipan::prelude::*;

const TRANSITION: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Iris,
    Wipe,
    Dissolve,
    Crossfade,
}

const STYLES: [Kind; 4] = [Kind::Iris, Kind::Wipe, Kind::Dissolve, Kind::Crossfade];

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Iris => "iris",
            Self::Wipe => "wipe",
            Self::Dissolve => "dissolve",
            Self::Crossfade => "crossfade",
        }
    }
}

/// Composites the scope's content (the incoming screen) over its backdrop (the outgoing one).
#[derive(Clone, Copy, Debug)]
struct Transition {
    style: Kind,
    progress: f32,
}

impl CellEffect for Transition {
    fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

    // Settled transitions composite nothing, so they skip the per-frame backdrop copy.
    fn uses_backdrop(&self) -> bool {
        self.progress < 1.0
    }

    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        let x = (ctx.x - ctx.bounds.x) as f32;
        let y = (ctx.y - ctx.bounds.y) as f32;
        let w = ctx.bounds.w.max(1) as f32;
        let h = ctx.bounds.h.max(1) as f32;
        let t = self.progress;
        match self.style {
            Kind::Iris => {
                // Rows count double so the iris reads as a circle rather than a tall ellipse.
                let reach = w.hypot(h * 2.0) * 0.5;
                let distance = (x - w * 0.5).hypot((y - h * 0.5) * 2.0) / reach;
                if distance > t {
                    *cell = backdrop.clone();
                } else if distance > t - 0.03 {
                    cell.set_symbol("·");
                    cell.set_fg(TerminalColor::Rgb(255, 214, 120));
                }
            }
            Kind::Wipe => {
                // A diagonal front with a soft edge a few cells wide.
                let position = (x / w + y / h * 0.35) / 1.35;
                let edge = 0.08;
                let alpha = ((t * (1.0 + edge) - position) / edge).clamp(0.0, 1.0);
                blend_into(cell, backdrop, alpha);
            }
            Kind::Dissolve => {
                if cell_noise(ctx.x, ctx.y) > t {
                    *cell = backdrop.clone();
                }
            }
            Kind::Crossfade => blend_into(cell, backdrop, t),
        }
    }
}

/// Mix `cell` (incoming, weight `alpha`) with `backdrop` (outgoing). The symbol follows whichever
/// side dominates; truecolor channels interpolate.
fn blend_into(cell: &mut EffectCell, backdrop: &EffectCell, alpha: f32) {
    if alpha >= 1.0 {
        return;
    }
    let incoming = cell.clone();
    if alpha < 0.5 {
        *cell = backdrop.clone();
    }
    cell.set_fg(mix(backdrop.fg, incoming.fg, alpha));
    cell.set_bg(mix(backdrop.bg, incoming.bg, alpha));
}

fn mix(from: TerminalColor, to: TerminalColor, t: f32) -> TerminalColor {
    match (from, to) {
        (TerminalColor::Rgb(r0, g0, b0), TerminalColor::Rgb(r1, g1, b1)) => {
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
            TerminalColor::Rgb(lerp(r0, r1), lerp(g0, g1), lerp(b0, b1))
        }
        _ if t < 0.5 => from,
        _ => to,
    }
}

/// Stable per-cell noise in `[0, 1)`, so the dissolve eats the same cells in the same order.
fn cell_noise(x: i16, y: i16) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65536.0
}

struct Screen {
    title: &'static str,
    bg: Color,
    fg: Color,
    lines: [&'static str; 3],
}

const SCREENS: [Screen; 4] = [
    Screen {
        title: "Dawn",
        bg: Color::rgb(46, 26, 58),
        fg: Color::rgb(255, 196, 150),
        lines: [
            "The outgoing screen is a plain ZStack layer.",
            "The incoming one sits in an EffectScope above it.",
            "Its effect picks a side for every cell.",
        ],
    },
    Screen {
        title: "Tide",
        bg: Color::rgb(12, 44, 66),
        fg: Color::rgb(140, 220, 255),
        lines: [
            "uses_backdrop() asks the renderer for a copy",
            "of the cells beneath the scope before it paints.",
            "apply_with_backdrop() receives both.",
        ],
    },
    Screen {
        title: "Moss",
        bg: Color::rgb(24, 52, 30),
        fg: Color::rgb(190, 240, 160),
        lines: [
            "*cell = backdrop.clone() lets the old screen show;",
            "leaving the cell alone keeps the new one;",
            "anything between is a blend.",
        ],
    },
    Screen {
        title: "Ember",
        bg: Color::rgb(60, 24, 14),
        fg: Color::rgb(255, 170, 110),
        lines: [
            "Once settled, uses_backdrop() reports false",
            "and the copy is skipped entirely.",
            "Press space to move on.",
        ],
    },
];

fn screen_view(index: usize, style: Kind) -> Element {
    let screen = &SCREENS[index];
    let text = Style::new().fg(screen.fg).bg(screen.bg);
    let mut body = VStack::new().gap(1).style(Style::new().bg(screen.bg));
    for line in screen.lines {
        body = body.child(Text::new(line).style(text));
    }
    Frame::new()
        .header_left(screen.title)
        .footer_left(format!(
            "space next screen | t transition: {} | q quit",
            style.label()
        ))
        .border(true)
        .padding(2)
        .style(text)
        .child(body)
        .into()
}

struct Transitions;

#[derive(Default)]
struct State {
    screen: usize,
    previous: usize,
    generation: u32,
    style: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    Next,
    CycleStyle,
}

impl Component for Transitions {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Next => {
                ctx.state.previous = ctx.state.screen;
                ctx.state.screen = (ctx.state.screen + 1) % SCREENS.len();
                ctx.state.generation += 1;
            }
            Msg::CycleStyle => ctx.state.style = (ctx.state.style + 1) % STYLES.len(),
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                ctx.link().send(Msg::Next);
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Char('t') => {
                ctx.link().send(Msg::CycleStyle);
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let style = STYLES[ctx.state.style];
        // One transition whose target is the switch count: each switch moves it up by one, and
        // how far it still has to go is how far the current transition has left.
        let generation = ctx.state.generation as f32;
        let value = ctx.transition(
            "screen-transition",
            generation,
            TransitionConfig {
                duration: TRANSITION,
                easing: Easing::EaseInOutCubic,
            },
        );
        let progress = (1.0 - (generation - value)).clamp(0.0, 1.0);

        // Keyed so the incoming layer is not remounted when the outgoing one leaves the stack.
        let incoming: Element = EffectScope::new()
            .custom_effect(Transition { style, progress })
            .child(screen_view(ctx.state.screen, style))
            .into();
        let mut stack = ZStack::new();
        if progress < 1.0 {
            stack = stack.child(screen_view(ctx.state.previous, style).key("outgoing"));
        }
        stack.child(incoming.key("incoming")).into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Backdrop Transitions")
        .mount(Transitions)
        .run()
}
