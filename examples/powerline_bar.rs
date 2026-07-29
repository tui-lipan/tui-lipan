//! Compact, interlocked powerline chains built with `Badge` segment caps.
//!
//! Run with `cargo run --example powerline_bar`. Press `n` to toggle Nerd Font
//! glyphs (`Round`/`Arrow`) versus their font-safe `Half` fallback.

use tui_lipan::prelude::*;

const BAR_BG: Color = Color::Rgb(24, 29, 39);
const TEXT_DARK: Color = Color::Rgb(15, 18, 24);
const COLORS: [Color; 4] = [
    Color::Rgb(80, 156, 220),
    Color::Rgb(92, 200, 155),
    Color::Rgb(229, 179, 88),
    Color::Rgb(194, 120, 214),
];

struct PowerlineBar;

struct State {
    nerd_font: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    ToggleFont,
}

impl Component for PowerlineBar {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State { nerd_font: true }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ToggleFont => ctx.state.nerd_font = !ctx.state.nerd_font,
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('n') => {
                self.update(Msg::ToggleFont, ctx);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let font_mode = if ctx.state.nerd_font {
            "Nerd Font caps enabled"
        } else {
            "font-safe fallback enabled"
        };

        Frame::new()
            .header_left("Powerline segment chains")
            .footer_left("n toggle Nerd Font fallback • q quit")
            .border(true)
            .padding(1)
            .style(Style::new().bg(BAR_BG))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(format!(
                        "{font_mode}. Each zero-gap cap is painted over the previous segment color."
                    )))
                    .child(style_demo("Padded", CapStyle::Padded, ctx.state.nerd_font))
                    .child(style_demo("Half", CapStyle::Half, ctx.state.nerd_font))
                    .child(style_demo("Round", CapStyle::Round, ctx.state.nerd_font))
                    .child(style_demo("Arrow", CapStyle::Arrow, ctx.state.nerd_font))
                    .child(same_color_demo(ctx.state.nerd_font))
                    .child(standalone_demo(ctx.state.nerd_font))
                    .child(side_demo(ctx.state.nerd_font)),
            )
            .into()
    }
}

fn style_demo(name: &'static str, style: CapStyle, nerd_font: bool) -> Element {
    let style = if nerd_font { style } else { style.font_safe() };
    HStack::new()
        .gap(1)
        .height(Length::Px(1))
        .child(Text::new(format!("{name:>7}")).width(Length::Px(7)))
        .child(powerline_chain(style))
        .into()
}

/// A real powerline chain: no gaps, and every left cap uses the previous
/// segment's background so the cells visually interlock.
fn powerline_chain(style: CapStyle) -> Element {
    let segments = [" MAIN ", " +3 ", " RUST ", " READY "];
    let mut previous_bg = BAR_BG;
    let mut chain = HStack::new()
        .gap(0)
        .width(Length::Auto)
        .height(Length::Px(1));
    for (index, label) in segments.into_iter().enumerate() {
        let background = COLORS[index];
        chain = chain.child(inline_badge(
            label,
            background,
            previous_bg,
            style,
            CapSides::Left,
            false,
        ));
        previous_bg = background;
    }
    chain.into()
}

fn same_color_demo(nerd_font: bool) -> Element {
    let style = if nerd_font {
        CapStyle::Arrow
    } else {
        CapStyle::Arrow.font_safe()
    };
    let labels = [" ONE ", " TWO ", " THREE "];
    let background = COLORS[0];
    let mut previous_bg = BAR_BG;
    let mut chain = HStack::new()
        .gap(0)
        .width(Length::Auto)
        .height(Length::Px(1));
    for (index, label) in labels.into_iter().enumerate() {
        let same_color = index > 0 && previous_bg == background;
        chain = chain.child(inline_badge(
            label,
            background,
            previous_bg,
            style,
            CapSides::Left,
            same_color,
        ));
        previous_bg = background;
    }
    HStack::new()
        .gap(1)
        .height(Length::Px(1))
        .child(Text::new("   Same").width(Length::Px(7)))
        .child(chain)
        .into()
}

fn standalone_demo(nerd_font: bool) -> Element {
    let style = if nerd_font {
        CapStyle::Round
    } else {
        CapStyle::Round.font_safe()
    };
    HStack::new()
        .gap(1)
        .height(Length::Px(1))
        .child(Text::new("  Pills").width(Length::Px(7)))
        .child(inline_badge(
            " BUILD ",
            COLORS[2],
            BAR_BG,
            style,
            CapSides::Both,
            false,
        ))
        .child(inline_badge(
            " TEST ",
            COLORS[0],
            BAR_BG,
            style,
            CapSides::Both,
            false,
        ))
        .child(inline_badge(
            " SHIP ",
            COLORS[1],
            BAR_BG,
            style,
            CapSides::Both,
            false,
        ))
        .into()
}

fn side_demo(nerd_font: bool) -> Element {
    let style = if nerd_font {
        CapStyle::Arrow
    } else {
        CapStyle::Arrow.font_safe()
    };
    HStack::new()
        .gap(1)
        .height(Length::Px(1))
        .child(Text::new("  Sides").width(Length::Px(7)))
        .child(inline_badge(
            " LEFT ",
            COLORS[3],
            BAR_BG,
            style,
            CapSides::Left,
            false,
        ))
        .child(inline_badge(
            " RIGHT ",
            COLORS[3],
            BAR_BG,
            style,
            CapSides::Right,
            false,
        ))
        .child(inline_badge(
            " NONE ",
            COLORS[3],
            BAR_BG,
            style,
            CapSides::None,
            false,
        ))
        .into()
}

/// `Badge` is an overlay widget, so an inline use gives its child an exact
/// one-row footprint. This keeps the outer ZStack content-sized instead of
/// allowing it to consume a flex share of the bar.
fn inline_badge(
    label: &'static str,
    background: Color,
    cap_behind: Color,
    cap: CapStyle,
    sides: CapSides,
    same_color: bool,
) -> Element {
    // Auto-sized caps replace the label's edge spaces, preserving its footprint.
    let footprint = label.chars().count() as u16;
    let badge = Badge::new(label)
        .child(
            Spacer::new()
                .width(Length::Px(footprint))
                .height(Length::Px(1)),
        )
        .position(BadgePosition::TopStart)
        .style(Style::new().bg(background))
        .text_style(Style::new().fg(TEXT_DARK).bold())
        .width(Length::Auto)
        .height(Length::Px(1))
        .cap(cap)
        .cap_behind(cap_behind)
        .cap_sides(sides)
        .cap_same_color(same_color);
    Frame::new()
        .border(false)
        .padding(0)
        .width(Length::Px(footprint))
        .height(Length::Px(1))
        .child(badge)
        .into()
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Powerline Bar")
        .terminal_bg(Some(BAR_BG))
        .mount(PowerlineBar)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    #[test]
    fn powerline_segments_render_as_compact_zero_gap_chains() {
        let mut backend = TestBackend::new(PowerlineBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 20,
        });
        backend.render();
        let frame = backend.capture_frame();
        let grid = frame.to_fixed_grid();

        assert!(grid.contains("Padded  MAIN  +3  RUST  READY"));
        assert!(grid.contains("Half ▐MAIN ▐+3 ▐RUST ▐READY"));
        assert!(grid.contains("Round MAIN +3 RUST READY"));
        assert!(grid.contains("Arrow MAIN +3 RUST READY"));
        assert!(grid.contains("Same ONE TWO THREE"));

        let arrow_y = frame
            .to_fixed_grid_lines()
            .iter()
            .position(|line| line.contains("Arrow MAIN"))
            .unwrap() as u16;
        let arrows: Vec<_> = (0..frame.width)
            .filter(|&x| frame.cell(x, arrow_y).symbol == "")
            .collect();
        assert_eq!(arrows.len(), 4);
        assert_eq!(frame.cell(arrows[0], arrow_y).fg, COLORS[0]);
        assert_eq!(frame.cell(arrows[0], arrow_y).bg, BAR_BG);
        assert_eq!(frame.cell(arrows[1], arrow_y).fg, COLORS[1]);
        assert_eq!(frame.cell(arrows[1], arrow_y).bg, COLORS[0]);

        let same_y = frame
            .to_fixed_grid_lines()
            .iter()
            .position(|line| line.contains("Same ONE"))
            .unwrap() as u16;
        let separators: Vec<_> = (0..frame.width)
            .filter(|&x| frame.cell(x, same_y).symbol == "")
            .collect();
        assert_eq!(separators.len(), 2);
        assert_eq!(frame.cell(separators[0], same_y).bg, COLORS[0]);
        assert_ne!(frame.cell(separators[0], same_y).fg, COLORS[0]);
    }

    #[test]
    fn nerd_font_fallback_uses_plain_padded_badges() {
        let mut backend = TestBackend::new(PowerlineBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 20,
        });
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char('n'),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
        let grid = backend.capture_frame().to_fixed_grid();

        assert!(grid.contains("Round  MAIN  +3  RUST  READY"));
        assert!(grid.contains("Arrow  MAIN  +3  RUST  READY"));
        assert!(grid.contains("Same  ONE ▏TWO ▏THREE"));
        assert!(!grid.contains(''));
        assert!(!grid.contains(''));
        assert!(!grid.contains(''));
        assert_eq!(grid.matches('▏').count(), 2);
    }
}
