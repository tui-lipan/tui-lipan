use std::time::Duration;

use tui_lipan::prelude::*;

struct SparklineShowcase;

#[derive(Default)]
struct State {
    tick: u64,
    live_usage: Vec<u64>,
    live_net_dl: Vec<u64>,
    live_net_ul: Vec<u64>,
}

#[derive(Clone)]
enum Msg {
    Tick,
}

impl Component for SparklineShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            tick: 0,
            live_usage: (0..28).map(sample_usage).collect(),
            live_net_dl: (0..28).map(sample_net_dl).collect(),
            live_net_ul: (0..28).map(sample_net_ul).collect(),
        }
    }

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        Some(schedule_tick())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Tick => {
                ctx.state.tick = ctx.state.tick.saturating_add(1);
                let t = ctx.state.tick;

                ctx.state.live_usage.push(sample_usage(t));
                ctx.state.live_net_dl.push(sample_net_dl(t));
                ctx.state.live_net_ul.push(sample_net_ul(t));

                trim_history(&mut ctx.state.live_usage, 4096);
                trim_history(&mut ctx.state.live_net_dl, 4096);
                trim_history(&mut ctx.state.live_net_ul, 4096);

                Update::with_command(schedule_tick())
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let columns = match ctx.breakpoint(110, 180) {
            Breakpoint::Small => 1,
            Breakpoint::Medium => 2,
            Breakpoint::Large => 3,
        };

        // ── static data ──────────────────────────────────────────────────────
        let cpu = vec![
            18u64, 35, 22, 48, 70, 64, 58, 40, 34, 29, 24, 18, 28, 45, 62, 77, 68, 51,
        ];
        let mem = vec![
            52u64, 51, 53, 54, 56, 58, 60, 63, 65, 64, 62, 61, 59, 57, 55, 54, 53, 52,
        ];
        let noisy = vec![
            8u64, 80, 16, 74, 22, 67, 29, 60, 35, 52, 41, 47, 39, 50, 33, 58,
        ];
        let spikes = vec![0u64, 0, 30, 65, 20, 80, 15, 15, 75, 22, 5, 50, 0, 0];
        let ul_like = vec![8u64, 10, 9, 8, 35, 12, 9, 8, 24, 10, 9, 8, 16, 10];

        let dense: Vec<u64> = (0..160)
            .map(|i| {
                let wave = (((i as f64) / 7.0).sin() * 22.0 + 40.0).round() as i32;
                let pulse = if i % 19 == 0 { 30 } else { 0 };
                (wave + pulse).clamp(0, 100) as u64
            })
            .collect();

        // ── Bars Presets + Zero Policy ────────────────────────────────────────
        let bars_frame = Frame::new()
            .title("Bars Presets + Zero Policy")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Blocks + Height Gradient").style(Style::new().bold()))
                    .child(
                        Sparkline::new(cpu.clone())
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .bars_preset(SparklineBarsPreset::Blocks)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(46, 204, 113),
                                Color::Rgb(52, 152, 219),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Shades + Yellow style").style(Style::new().bold()))
                    .child(
                        Sparkline::new(mem)
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .bars_preset(SparklineBarsPreset::Shades)
                            .style(Style::new().fg(Color::Yellow))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("ZeroPolicy::Empty (default) vs MinGlyph")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new([0u64, 5, 0, 20, 0, 50, 0, 80, 0, 100, 0, 60, 0, 10, 0])
                            .min(0)
                            .max(100)
                            .chart_height(2)
                            .zero_policy(SparklineZeroPolicy::Empty)
                            .style(Style::new().fg(Color::Rgb(100, 200, 255)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Sparkline::new([0u64, 5, 0, 20, 0, 50, 0, 80, 0, 100, 0, 60, 0, 10, 0])
                            .min(0)
                            .max(100)
                            .chart_height(2)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .style(Style::new().fg(Color::Rgb(255, 180, 100)))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Braille Variants ──────────────────────────────────────────────────
        let braille_frame = Frame::new()
            .title("Braille Variants")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Pair-packed spikes").style(Style::new().bold()))
                    .child(
                        Sparkline::new(spikes.clone())
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("Mirrored UL-like baseline + gradient")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ul_like.clone())
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .mirror_y(true)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .gradient(ColorGradient::new(
                                Color::Rgb(155, 89, 182),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Braille + height gradient").style(Style::new().bold()))
                    .child(
                        Sparkline::new(spikes)
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(52, 152, 219),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Line Variants ─────────────────────────────────────────────────────
        let line_frame = Frame::new()
            .title("Line Variants")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Unicode line (multi-row)").style(Style::new().bold()))
                    .child(
                        Sparkline::new(noisy.clone())
                            .line()
                            .min(0)
                            .max(100)
                            .chart_height(5)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(52, 152, 219),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("mirror_x + per-trend styles").style(Style::new().bold()))
                    .child(
                        Sparkline::new(noisy.clone())
                            .line()
                            .min(0)
                            .max(100)
                            .chart_height(5)
                            .mirror_x(true)
                            .turn_style(Style::new().fg(Color::Rgb(241, 196, 15)).bold())
                            .rising_style(Style::new().fg(Color::Rgb(46, 204, 113)))
                            .falling_style(Style::new().fg(Color::Rgb(231, 76, 60)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("ASCII preset - single-row trend glyphs (/\\-^v)")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(noisy.clone())
                            .line()
                            .line_preset(SparklineLinePreset::Ascii)
                            .min(0)
                            .max(100)
                            .chart_height(1)
                            .style(Style::new().fg(Color::Rgb(180, 180, 180)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("ASCII preset - multi-row grid (works best on smooth data)")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(cpu.clone())
                            .line()
                            .line_preset(SparklineLinePreset::Ascii)
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .style(Style::new().fg(Color::Rgb(180, 180, 180)))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Gradients ─────────────────────────────────────────────────────────
        let gradient_frame = Frame::new()
            .title("Gradients")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Value gradient (2-stop)").style(Style::new().bold()))
                    .child(
                        Sparkline::new([0u64, 5, 10, 30, 55, 80, 100, 90, 60, 30, 10, 0])
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .gradient(ColorGradient::new(
                                Color::Rgb(40, 180, 99),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Value gradient (3-stop)").style(Style::new().bold()))
                    .child(
                        Sparkline::new([0u64, 20, 35, 50, 65, 80, 95, 80, 55, 35, 20, 0])
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .gradient(
                                ColorGradient::new(
                                    Color::Rgb(52, 152, 219),
                                    Color::Rgb(231, 76, 60),
                                )
                                .with_center(Color::Rgb(241, 196, 15)),
                            )
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Height gradient (row-based)").style(Style::new().bold()))
                    .child(
                        Sparkline::new([10u64, 20, 45, 70, 100, 70, 45, 20, 10])
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .bars_preset(SparklineBarsPreset::Blocks)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(46, 204, 113),
                                Color::Rgb(41, 128, 185),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Line + gradient + custom range").style(Style::new().bold()))
                    .child(
                        Sparkline::new([10u64, 30, 60, 90, 70, 40, 20, 50, 80, 100, 60, 30])
                            .line()
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .gradient(ColorGradient::new(
                                Color::Rgb(52, 152, 219),
                                Color::Rgb(155, 89, 182),
                            ))
                            .gradient_range(0, 100)
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Aggregation Strategies ────────────────────────────────────────────
        let aggregation_frame = Frame::new()
            .title("Downsampling: All Aggregation Strategies")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Average (smooth)").style(Style::new().bold()))
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(40)
                            .aggregation(SparklineAggregation::Average)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .style(Style::new().fg(Color::Rgb(52, 152, 219)))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Max (spike-preserving)").style(Style::new().bold()))
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(40)
                            .aggregation(SparklineAggregation::Max)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .style(Style::new().fg(Color::Rgb(231, 76, 60)))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("Min (trough-tracking)").style(Style::new().bold()))
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(40)
                            .aggregation(SparklineAggregation::Min)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .style(Style::new().fg(Color::Rgb(46, 204, 113)))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("First / Last per bucket").style(Style::new().bold()))
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(40)
                            .aggregation(SparklineAggregation::First)
                            .chart_height(2)
                            .min(0)
                            .max(100)
                            .style(Style::new().fg(Color::Rgb(241, 196, 15)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(40)
                            .aggregation(SparklineAggregation::Last)
                            .chart_height(2)
                            .min(0)
                            .max(100)
                            .style(Style::new().fg(Color::Rgb(155, 89, 182)))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Mirroring ─────────────────────────────────────────────────────────
        let mirror_frame = Frame::new()
            .title("Mirroring")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new("Normal (oldest left, newest right)").style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(48)
                            .aggregation(SparklineAggregation::Max)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("mirror_x (newest left, oldest right)")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(48)
                            .aggregation(SparklineAggregation::Max)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .mirror_x(true)
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("mirror_y - braille (fully mirrored, dots flip per cell)")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(dense.clone())
                            .variant(SparklineVariant::Braille)
                            .max_points(96)
                            .aggregation(SparklineAggregation::Max)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .mirror_y(true)
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new(
                            "mirror_y - bars: rows flip, partial-fill still bottom-up (see docs)",
                        )
                        .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(dense.clone())
                            .max_points(48)
                            .aggregation(SparklineAggregation::Max)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .mirror_y(true)
                            .style(Style::new().fg(Color::Rgb(180, 180, 180)))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("mirror_x + mirror_y (line)").style(Style::new().bold()))
                    .child(
                        Sparkline::new(dense)
                            .max_points(48)
                            .aggregation(SparklineAggregation::Max)
                            .variant(SparklineVariant::Line)
                            .chart_height(3)
                            .min(0)
                            .max(100)
                            .mirror_x(true)
                            .mirror_y(true)
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Live Stream ───────────────────────────────────────────────────────
        let live_frame = Frame::new()
            .title("Live Stream")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new("CPU usage - Line + height gradient").style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .variant(SparklineVariant::Line)
                            .min(0)
                            .max(100)
                            .chart_height(5)
                            .overflow(Overflow::ClipStart)
                            .height_gradient(
                                ColorGradient::new(
                                    Color::Rgb(80, 120, 255),
                                    Color::Rgb(255, 107, 53),
                                )
                                .with_center(Color::Rgb(72, 201, 176)),
                            )
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("CPU usage - Bars + height gradient").style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .variant(SparklineVariant::Bars)
                            .min(0)
                            .max(100)
                            .chart_height(4)
                            .bars_preset(SparklineBarsPreset::Blocks)
                            .overflow(Overflow::ClipStart)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(46, 204, 113),
                                Color::Rgb(41, 128, 185),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("DL - Braille + ZeroPolicy::MinGlyph").style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_net_dl.clone())
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .overflow(Overflow::ClipStart)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .style(Style::new().fg(Color::Rgb(52, 152, 219)))
                            .width(Length::Flex(1)),
                    )
                    .child(Text::new("UL - Braille mirrored + gradient").style(Style::new().bold()))
                    .child(
                        Sparkline::new(ctx.state.live_net_ul.clone())
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .mirror_y(true)
                            .overflow(Overflow::ClipStart)
                            .zero_policy(SparklineZeroPolicy::MinGlyph)
                            .gradient(ColorGradient::new(
                                Color::Rgb(46, 204, 113),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Overflow Modes (Live) ─────────────────────────────────────────────
        let overflow_frame = Frame::new()
            .title("Overflow Modes (Live - same data, different strategy)")
            .border(true)
            .width(Length::Flex(1))
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new("Auto / ClipStart - newest samples at the right edge (scroll)")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .overflow(Overflow::Auto)
                            .height_gradient(ColorGradient::new(
                                Color::Rgb(46, 204, 113),
                                Color::Rgb(52, 152, 219),
                            ))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("Wrap - full history bucket-averaged to width")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .overflow(Overflow::Wrap)
                            .style(Style::new().fg(Color::Rgb(241, 196, 15)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("Clip / Ellipsis - oldest samples frozen at the left edge")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .overflow(Overflow::Clip)
                            .style(Style::new().fg(Color::Rgb(231, 76, 60)))
                            .width(Length::Flex(1)),
                    )
                    .child(
                        Text::new("Wrap - braille, full history compressed")
                            .style(Style::new().bold()),
                    )
                    .child(
                        Sparkline::new(ctx.state.live_usage.clone())
                            .variant(SparklineVariant::Braille)
                            .min(0)
                            .max(100)
                            .chart_height(3)
                            .overflow(Overflow::Wrap)
                            .aggregation(SparklineAggregation::Max)
                            .gradient(ColorGradient::new(
                                Color::Rgb(155, 89, 182),
                                Color::Rgb(231, 76, 60),
                            ))
                            .width(Length::Flex(1)),
                    ),
            );

        // ── Grid ──────────────────────────────────────────────────────────────
        let showcase_grid = Grid::new()
            .uniform_columns(columns)
            .column_gap(1)
            .row_gap(1)
            .width(Length::Flex(1))
            .child(bars_frame)
            .child(braille_frame)
            .child(line_frame)
            .child(gradient_frame)
            .child(aggregation_frame)
            .child(mirror_frame)
            .child(live_frame)
            .child(overflow_frame);

        let scroll = VStack::new().height(Length::Flex(1)).child(
            ScrollView::new()
                .padding(1)
                .gap(1)
                .scrollbar(true)
                .scroll_keys(ScrollKeymap::DEFAULT)
                .show_scroll_indicators(true)
                .child(showcase_grid),
        );

        VStack::new()
            .gap(1)
            .padding(1)
            .child(Text::new("Sparkline Showcase").style(Style::new().bold().fg(Color::Cyan)))
            .child(scroll)
            .child(
                StatusBar::new()
                    .style(Style::new().bg(Color::indexed(236)).fg(Color::indexed(252)))
                    .left(Text::new(
                        "Sparkline: variants · gradients · aggregation · overflow · live",
                    ))
                    .right(Text::new("q: quit | wheel/pgup/pgdn: scroll")),
            )
            .into()
    }
}

fn sample_usage(tick: u64) -> u64 {
    let wave = (((tick as f64) / 9.0).sin() * 20.0 + 46.0).round() as i32;
    let pulse = if matches!(tick % 41, 9 | 10) {
        28
    } else if tick % 67 == 23 {
        18
    } else {
        0
    };
    (wave + pulse).clamp(0, 100) as u64
}

fn sample_net_dl(tick: u64) -> u64 {
    let base = 32 + (((tick as f64) / 12.0).sin() * 3.0).round() as i32;
    let burst = if matches!(tick % 31, 7 | 8) {
        36
    } else if tick % 53 == 19 {
        24
    } else {
        0
    };
    (base + burst).clamp(0, 100) as u64
}

fn sample_net_ul(tick: u64) -> u64 {
    let base = 10 + (((tick as f64) / 10.0).cos() * 2.0).round() as i32;
    let burst = if matches!(tick % 23, 12 | 13) {
        18
    } else if tick % 37 == 4 {
        24
    } else {
        0
    };
    (base + burst).clamp(0, 100) as u64
}

fn trim_history(values: &mut Vec<u64>, keep: usize) {
    if values.len() > keep {
        let drop = values.len() - keep;
        values.drain(0..drop);
    }
}

fn schedule_tick() -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(Duration::from_millis(120));
        link.send(Msg::Tick);
    })
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Sparkline Showcase")
        .mount(SparklineShowcase)
        .run()
}
