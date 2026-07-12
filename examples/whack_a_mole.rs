//! Terminal whack-a-mole: `Frame`/`HStack`/`VStack` layout, `MouseRegion` per-hole hits,
//! `ProgressBar` health, `Sparkline` hit pace, `Spinner`, `Modal` on game over, and buttons with shortcuts.
// Kept in sync with tui-lipan/website `wasm/showcase` (`WhackAMole` / `WebControlsHandle`).

use std::collections::VecDeque;
use std::time::Duration;

use tui_lipan::prelude::*;

/// Game tick interval (~20 FPS). Mole lifetimes are counted in ticks.
const FRAME_MS: u64 = 50;

fn next_frame_cmd() -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(Duration::from_millis(FRAME_MS));
        link.send(Msg::Tick);
    })
}

fn with_frame(mut u: Update) -> Update {
    u.command = Some(next_frame_cmd());
    u
}

const MOLE_GRID: usize = 9;
const MOLE_SPARK_BUCKETS: usize = 36;
const MOLE_SPARK_BUCKET_TICKS: u64 = 2;

#[derive(Clone, Copy, PartialEq)]
enum MoleKind {
    Normal,
    Golden,
    Bomb,
}

#[derive(Clone, Copy)]
struct Mole {
    spawned_at: u64,
    lifetime: u32,
    kind: MoleKind,
}

#[derive(Clone, Copy, PartialEq)]
enum GamePhase {
    Idle,
    Playing,
    Paused,
    GameOver,
}

struct State {
    holes: [Option<Mole>; MOLE_GRID],
    phase: GamePhase,
    score: u32,
    best_score: u32,
    health: i32,
    streak: u32,
    best_streak: u32,
    hits: u32,
    misses: u32,
    tick: u64,
    bucket_hits: u64,
    history: VecDeque<u64>,
    rng: u64,
    flash: [Option<(MoleKind, u64)>; MOLE_GRID],
    last_event: Option<(LastEvent, u64)>,
}

#[derive(Clone, Copy)]
enum LastEvent {
    Hit(u32),
    Golden(u32),
    Miss,
    Bomb,
}

impl Default for State {
    fn default() -> Self {
        Self {
            holes: [None; MOLE_GRID],
            phase: GamePhase::Idle,
            score: 0,
            best_score: 0,
            health: 100,
            streak: 0,
            best_streak: 0,
            hits: 0,
            misses: 0,
            tick: 0,
            bucket_hits: 0,
            history: VecDeque::with_capacity(MOLE_SPARK_BUCKETS),
            rng: 0x9E37_79B9_7F4A_7C15,
            flash: [None; MOLE_GRID],
            last_event: None,
        }
    }
}

impl State {
    fn new() -> Self {
        Self::default()
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn rand_in(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.rand() % n
    }

    fn level(&self) -> u32 {
        1 + self.score / 120
    }

    fn active_moles(&self) -> usize {
        self.holes.iter().filter(|h| h.is_some()).count()
    }

    fn target_active(&self) -> usize {
        let lvl = self.level();
        ((lvl as usize).min(4) + 1).min(MOLE_GRID)
    }

    fn spawn_lifetime(&mut self) -> u32 {
        let lvl = self.level().min(8);
        let base = 28u32.saturating_sub(lvl * 2).max(11);
        let jitter = self.rand_in(8) as u32;
        base + jitter
    }

    fn spawn_chance(&self) -> u64 {
        let lvl = self.level().min(8) as u64;
        20 + lvl * 6
    }

    fn pick_kind(&mut self) -> MoleKind {
        let r = self.rand_in(100);
        if r < 6 {
            MoleKind::Bomb
        } else if r < 14 {
            MoleKind::Golden
        } else {
            MoleKind::Normal
        }
    }

    fn reset_round(&mut self) {
        self.holes = [None; MOLE_GRID];
        self.flash = [None; MOLE_GRID];
        self.score = 0;
        self.health = 100;
        self.streak = 0;
        self.hits = 0;
        self.misses = 0;
        self.tick = 0;
        self.bucket_hits = 0;
        self.history.clear();
        self.last_event = None;
    }

    fn record_event(&mut self, ev: LastEvent) {
        self.last_event = Some((ev, self.tick));
    }

    fn finalize(&mut self) {
        if self.score > self.best_score {
            self.best_score = self.score;
        }
        if self.streak > self.best_streak {
            self.best_streak = self.streak;
        }
    }
}

#[derive(Clone)]
enum Msg {
    Tick,
    Hit(u8),
    Start,
    Pause,
    Reset,
}

struct WhackAMole;

impl Component for WhackAMole {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::new()
    }

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        Some(next_frame_cmd())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        let attach_frame = matches!(&msg, Msg::Tick);
        match msg {
            Msg::Start => {
                if ctx.state.phase == GamePhase::GameOver {
                    ctx.state.reset_round();
                }
                ctx.state.phase = GamePhase::Playing;
            }
            Msg::Pause => {
                if ctx.state.phase == GamePhase::Playing {
                    ctx.state.phase = GamePhase::Paused;
                } else if ctx.state.phase == GamePhase::Paused {
                    ctx.state.phase = GamePhase::Playing;
                }
            }
            Msg::Reset => {
                ctx.state.finalize();
                ctx.state.reset_round();
                ctx.state.phase = GamePhase::Idle;
            }
            Msg::Hit(idx) => {
                if ctx.state.phase != GamePhase::Playing {
                    if ctx.state.phase == GamePhase::Idle || ctx.state.phase == GamePhase::GameOver
                    {
                        if ctx.state.phase == GamePhase::GameOver {
                            ctx.state.reset_round();
                        }
                        ctx.state.phase = GamePhase::Playing;
                    } else {
                        return Update::full();
                    }
                }
                let i = idx as usize;
                if i >= MOLE_GRID {
                    return Update::none();
                }
                if let Some(mole) = ctx.state.holes[i] {
                    match mole.kind {
                        MoleKind::Normal => {
                            ctx.state.streak = ctx.state.streak.saturating_add(1);
                            if ctx.state.streak > ctx.state.best_streak {
                                ctx.state.best_streak = ctx.state.streak;
                            }
                            let bonus = ctx.state.streak.min(15);
                            let pts = 10 + bonus;
                            ctx.state.score = ctx.state.score.saturating_add(pts);
                            ctx.state.hits = ctx.state.hits.saturating_add(1);
                            ctx.state.bucket_hits = ctx.state.bucket_hits.saturating_add(1);
                            ctx.state.flash[i] = Some((MoleKind::Normal, ctx.state.tick));
                            ctx.state.record_event(LastEvent::Hit(pts));
                        }
                        MoleKind::Golden => {
                            ctx.state.streak = ctx.state.streak.saturating_add(2);
                            if ctx.state.streak > ctx.state.best_streak {
                                ctx.state.best_streak = ctx.state.streak;
                            }
                            let pts = 35;
                            ctx.state.score = ctx.state.score.saturating_add(pts);
                            ctx.state.hits = ctx.state.hits.saturating_add(1);
                            ctx.state.bucket_hits = ctx.state.bucket_hits.saturating_add(2);
                            ctx.state.health = (ctx.state.health + 8).min(100);
                            ctx.state.flash[i] = Some((MoleKind::Golden, ctx.state.tick));
                            ctx.state.record_event(LastEvent::Golden(pts));
                        }
                        MoleKind::Bomb => {
                            ctx.state.streak = 0;
                            ctx.state.health -= 22;
                            ctx.state.misses = ctx.state.misses.saturating_add(1);
                            ctx.state.flash[i] = Some((MoleKind::Bomb, ctx.state.tick));
                            ctx.state.record_event(LastEvent::Bomb);
                        }
                    }
                    ctx.state.holes[i] = None;
                } else {
                    ctx.state.streak = 0;
                    ctx.state.health -= 3;
                    ctx.state.misses = ctx.state.misses.saturating_add(1);
                    ctx.state.record_event(LastEvent::Miss);
                }
                if ctx.state.health <= 0 {
                    ctx.state.health = 0;
                    ctx.state.finalize();
                    ctx.state.phase = GamePhase::GameOver;
                }
            }
            Msg::Tick => {
                ctx.state.tick = ctx.state.tick.wrapping_add(1);
                if ctx.state.phase != GamePhase::Playing {
                    return with_frame(Update::full());
                }

                let now = ctx.state.tick;
                let mut auto_misses: u32 = 0;
                for i in 0..MOLE_GRID {
                    if let Some(mole) = ctx.state.holes[i]
                        && now.saturating_sub(mole.spawned_at) >= mole.lifetime as u64
                    {
                        ctx.state.holes[i] = None;
                        if mole.kind != MoleKind::Bomb {
                            auto_misses += 1;
                        }
                    }
                    if let Some((_, t)) = ctx.state.flash[i]
                        && now.saturating_sub(t) > 4
                    {
                        ctx.state.flash[i] = None;
                    }
                }
                if auto_misses > 0 {
                    ctx.state.streak = 0;
                    ctx.state.misses = ctx.state.misses.saturating_add(auto_misses);
                    ctx.state.health -= (auto_misses as i32) * 6;
                    ctx.state.record_event(LastEvent::Miss);
                    if ctx.state.health <= 0 {
                        ctx.state.health = 0;
                        ctx.state.finalize();
                        ctx.state.phase = GamePhase::GameOver;
                        return with_frame(Update::full());
                    }
                }

                if ctx.state.tick.is_multiple_of(MOLE_SPARK_BUCKET_TICKS) {
                    let v = ctx.state.bucket_hits;
                    ctx.state.bucket_hits = 0;
                    if ctx.state.history.len() >= MOLE_SPARK_BUCKETS {
                        ctx.state.history.pop_front();
                    }
                    ctx.state.history.push_back(v);
                }

                if ctx.state.active_moles() < ctx.state.target_active() {
                    let chance = ctx.state.spawn_chance();
                    if ctx.state.rand_in(100) < chance {
                        let mut empties: [u8; MOLE_GRID] = [0; MOLE_GRID];
                        let mut n = 0u8;
                        for i in 0..MOLE_GRID {
                            if ctx.state.holes[i].is_none() {
                                empties[n as usize] = i as u8;
                                n += 1;
                            }
                        }
                        if n > 0 {
                            let pick = ctx.state.rand_in(n as u64) as usize;
                            let slot = empties[pick] as usize;
                            let kind = ctx.state.pick_kind();
                            let lifetime = ctx.state.spawn_lifetime();
                            ctx.state.holes[slot] = Some(Mole {
                                spawned_at: now,
                                lifetime,
                                kind,
                            });
                        }
                    }
                }
            }
        }
        if attach_frame {
            with_frame(Update::full())
        } else {
            Update::full()
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let s = &ctx.state;
        let now = s.tick;
        let phase = s.phase;

        let accent = Style::default().fg(Color::rgb(192, 132, 252)).bold();
        let cyan = Style::default().fg(Color::rgb(34, 211, 238)).bold();
        let green = Style::default().fg(Color::rgb(34, 197, 94)).bold();
        let amber = Style::default().fg(Color::rgb(251, 191, 36)).bold();
        let pink = Style::default().fg(Color::rgb(244, 114, 182)).bold();
        let red = Style::default().fg(Color::rgb(239, 68, 68)).bold();
        let muted = Style::default().fg(Color::rgb(148, 163, 184));
        let dim = Style::default().fg(Color::rgb(71, 85, 105));
        let panel = Style::default().bg(Color::rgb(10, 15, 28));
        let rail = Style::default().fg(Color::rgb(51, 65, 85));

        let title_text = match phase {
            GamePhase::Idle => " whack-a-mole · press SPACE or click to start ",
            GamePhase::Playing => " whack-a-mole · LIVE ",
            GamePhase::Paused => " whack-a-mole · paused ",
            GamePhase::GameOver => " whack-a-mole · GAME OVER ",
        };

        let stats_panel = Frame::new()
            .title(" stats ")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .title_style(accent)
            .inner_style(panel)
            .width(Length::Flex(1))
            .height(Length::Px(11))
            .child(
                VStack::new()
                    .gap(0)
                    .padding((0, 1, 0, 1))
                    .style(panel)
                    .child(stat_row("score", format!("{}", s.score), cyan, muted))
                    .child(stat_row("best", format!("{}", s.best_score), accent, muted))
                    .child(stat_row("level", format!("{}", s.level()), green, muted))
                    .child(stat_row(
                        "streak",
                        format!("{}×", s.streak),
                        if s.streak >= 5 { pink } else { muted },
                        muted,
                    ))
                    .child(stat_row(
                        "best ×",
                        format!("{}", s.best_streak),
                        muted,
                        muted,
                    ))
                    .child(stat_row("hits", format!("{}", s.hits), green, muted))
                    .child(stat_row(
                        "misses",
                        format!("{}", s.misses),
                        if s.misses > 0 { amber } else { muted },
                        muted,
                    )),
            );

        let pace_max = s.history.iter().copied().max().unwrap_or(0).max(4);

        let event_line = match s.last_event {
            None => Text::new("ready when you are").style(muted),
            Some((LastEvent::Hit(p), _)) => Text::new(format!("+{} hit", p)).style(green),
            Some((LastEvent::Golden(p), _)) => {
                Text::new(format!("+{} GOLDEN · +heal", p)).style(amber)
            }
            Some((LastEvent::Miss, _)) => Text::new("miss · streak broken").style(red),
            Some((LastEvent::Bomb, _)) => Text::new("BOMB · -22 hp").style(red),
        };

        let pace_panel = Frame::new()
            .title(" pace ")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .title_style(accent)
            .inner_style(panel)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .padding((0, 1))
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .style(panel)
                    .child(
                        Sparkline::new(s.history.iter().copied())
                            .variant(SparklineVariant::Line)
                            .min(0)
                            .max(pace_max)
                            .chart_height(6)
                            .overflow(Overflow::ClipStart)
                            .height_gradient(
                                ColorGradient::new(
                                    Color::Rgb(168, 85, 247),
                                    Color::Rgb(236, 72, 153),
                                )
                                .with_center(Color::Rgb(139, 92, 246)),
                            )
                            .width(Length::Flex(1)),
                    )
                    .child(
                        HStack::new()
                            .gap(1)
                            .align(Align::Center)
                            .justify(Justify::Center)
                            .child(
                                Spinner::new()
                                    .spinner_style(SpinnerStyle::Braille)
                                    .speed(SpinnerSpeed::Normal)
                                    .frame(now as usize)
                                    .style(if phase == GamePhase::Playing {
                                        green
                                    } else {
                                        dim
                                    }),
                            )
                            .child(
                                Text::new(match phase {
                                    GamePhase::Idle => "press start to begin",
                                    GamePhase::Playing => "moles incoming",
                                    GamePhase::Paused => "paused",
                                    GamePhase::GameOver => "game over",
                                })
                                .style(muted),
                            ),
                    )
                    .child(event_line),
            );

        let left_column = VStack::new()
            .gap(1)
            .width(Length::Px(28))
            .height(Length::Flex(1))
            .child(stats_panel)
            .child(pace_panel);

        let mut mole_grid = Grid::new()
            .uniform_columns(3)
            .rows([Length::Auto, Length::Auto, Length::Auto])
            .gap_x(2)
            .gap_y(0)
            .width(Length::Auto)
            .height(Length::Auto)
            .style(panel)
            .align(Align::Center)
            .justify(Justify::Center);
        for row in 0..3 {
            for col in 0..3 {
                let idx = row * 3 + col;
                mole_grid = mole_grid.cell(
                    row as u16,
                    col as u16,
                    mole_cell(
                        ctx,
                        idx,
                        s.holes[idx],
                        s.flash[idx],
                        now,
                        phase,
                        accent,
                        cyan,
                        pink,
                        amber,
                        red,
                        muted,
                        dim,
                    ),
                );
            }
        }

        let grid_panel = Frame::new()
            .title(" targets ")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .title_style(accent)
            .inner_style(panel)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .child_align(Align::Center)
            .child(mole_grid);

        let health_ratio = (s.health as f64 / 100.0).clamp(0.0, 1.0);
        let health_style = if s.health > 60 {
            green
        } else if s.health > 30 {
            amber
        } else {
            red
        };
        let health_bar = ProgressBar::new(health_ratio)
            .label("health")
            .percentage_position(ProgressTextPosition::Above)
            .label_position(ProgressTextPosition::Above)
            .progress_style(ProgressStyle::Rect)
            .filled_style(health_style)
            .empty_style(rail)
            .show_percentage(true);

        let action_label = match phase {
            GamePhase::Idle => "start",
            GamePhase::Playing => "running",
            GamePhase::Paused => "resume",
            GamePhase::GameOver => "play again",
        };
        let button_style = Style::default().fg(Color::rgb(192, 132, 252)).bold();

        let action_row = HStack::new()
            .gap(2)
            .justify(Justify::Center)
            .child(
                Button::outlined(action_label)
                    .shortcut("space")
                    .style(button_style)
                    .focusable(false)
                    .on_click(ctx.link().callback(|_| Msg::Start)),
            )
            .child(
                Button::outlined("pause")
                    .shortcut("p")
                    .style(button_style)
                    .focusable(false)
                    .on_click(ctx.link().callback(|_| Msg::Pause)),
            )
            .child(
                Button::outlined("reset")
                    .shortcut("r")
                    .style(button_style)
                    .focusable(false)
                    .on_click(ctx.link().callback(|_| Msg::Reset)),
            );

        let right_column = VStack::new().gap(1).child(grid_panel).child(
            VStack::new()
                .gap(1)
                .height(Length::Auto)
                .child(health_bar)
                .child(action_row),
        );

        let console = Frame::new()
            .title(title_text)
            .status_right("click moles · 1-9 keys · space play/pause · r reset")
            .border(true)
            .height(Length::Flex(1))
            .width(Length::Flex(1))
            .border_style(BorderStyle::Rounded)
            .title_style(accent)
            .status_style(muted)
            .inner_style(panel)
            .child(
                HStack::new()
                    .gap(1)
                    .padding(1)
                    .align(Align::Stretch)
                    .style(panel)
                    .child(left_column)
                    .child(right_column),
            );

        let mut layers = ZStack::new().child(console);
        if phase == GamePhase::GameOver {
            layers = layers.child(
                Modal::new()
                    .scope(OverlayScope::Local)
                    .title(" GAME OVER ")
                    .title_alignment(Align::Center)
                    .title_style(red)
                    .border_style(BorderStyle::Rounded)
                    .width(Length::Percent(60))
                    .height(Length::Percent(60))
                    .backdrop_style(Style::new().tint_by(Color::rgb(40, 0, 0), 0.55))
                    .frame_style(Style::default().bg(Color::rgb(15, 23, 42)))
                    .on_close(ctx.link().callback(|_| Msg::Reset))
                    .child(
                        VStack::new()
                            .gap(1)
                            .align(Align::Center)
                            .justify(Justify::Center)
                            .child(Text::new(format!("score   {}", s.score)).style(accent))
                            .child(Text::new(format!("best    {}", s.best_score)).style(cyan))
                            .child(Text::new(format!("streak  {}", s.best_streak)).style(muted))
                            .child(Text::new(" ").style(muted))
                            .child(
                                HStack::new()
                                    .gap(2)
                                    .justify(Justify::Center)
                                    .child(
                                        Button::outlined("↺ play again")
                                            .shortcut("space")
                                            .style(
                                                Style::default()
                                                    .fg(Color::rgb(192, 132, 252))
                                                    .bold(),
                                            )
                                            .focusable(false)
                                            .on_click(ctx.link().callback(|_| Msg::Start)),
                                    )
                                    .child(
                                        Button::outlined("close")
                                            .style(muted)
                                            .focusable(false)
                                            .on_click(ctx.link().callback(|_| Msg::Reset)),
                                    ),
                            ),
                    ),
            );
        }

        layers.into()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let _ = key.mods;
        match key.code {
            KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                let idx = (c as u8) - b'1';
                ctx.link().send(Msg::Hit(idx));
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char(' ') => {
                if ctx.state.phase == GamePhase::Playing {
                    ctx.link().send(Msg::Pause);
                } else {
                    ctx.link().send(Msg::Start);
                }
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                ctx.link().send(Msg::Pause);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                ctx.link().send(Msg::Reset);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Esc => {
                if ctx.state.phase == GamePhase::Playing {
                    ctx.link().send(Msg::Pause);
                }
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn stat_row(label: &str, value: String, value_style: Style, label_style: Style) -> Element {
    HStack::new()
        .gap(1)
        .child(
            Text::new(format!("{:<8}", label))
                .style(label_style)
                .width(Length::Px(9)),
        )
        .child(Text::new(value).style(value_style))
        .into()
}

#[allow(clippy::too_many_arguments)]
fn mole_cell(
    ctx: &Context<WhackAMole>,
    idx: usize,
    mole: Option<Mole>,
    flash: Option<(MoleKind, u64)>,
    now: u64,
    phase: GamePhase,
    accent: Style,
    cyan: Style,
    pink: Style,
    amber: Style,
    red: Style,
    muted: Style,
    dim: Style,
) -> Element {
    let _ = (cyan, accent);
    let key_label = format!("{}", idx + 1);

    let (label, label_style, border_style, frame_bg, full_label) = match (mole, flash) {
        (_, Some((MoleKind::Normal, t))) if now.saturating_sub(t) <= 3 => (
            "✦",
            Style::default().fg(Color::rgb(34, 197, 94)).bold(),
            BorderStyle::Rounded,
            Color::rgb(20, 60, 40),
            " hit ".to_string(),
        ),
        (_, Some((MoleKind::Golden, t))) if now.saturating_sub(t) <= 3 => (
            "★",
            amber,
            BorderStyle::Double,
            Color::rgb(70, 50, 10),
            " GOLD ".to_string(),
        ),
        (_, Some((MoleKind::Bomb, t))) if now.saturating_sub(t) <= 3 => (
            "✸",
            red,
            BorderStyle::Double,
            Color::rgb(70, 10, 10),
            " BOOM ".to_string(),
        ),
        (Some(m), _) => match m.kind {
            MoleKind::Normal => (
                "●",
                pink,
                BorderStyle::Rounded,
                Color::rgb(20, 14, 38),
                format!(" {} ", key_label),
            ),
            MoleKind::Golden => (
                "★",
                amber,
                BorderStyle::Double,
                Color::rgb(40, 28, 6),
                format!(" {} ", key_label),
            ),
            MoleKind::Bomb => (
                "✸",
                red,
                BorderStyle::Double,
                Color::rgb(40, 10, 10),
                format!(" {} ", key_label),
            ),
        },
        (None, _) => (
            if phase == GamePhase::Playing {
                "·"
            } else {
                " "
            },
            dim,
            BorderStyle::Plain,
            Color::rgb(10, 15, 28),
            format!(" {} ", key_label),
        ),
    };

    let _ = muted;
    let cell_idx = idx as u8;
    let inner = VStack::new()
        .align(Align::Center)
        .justify(Justify::Center)
        .style(Style::default().bg(frame_bg))
        .child(Text::new(label).style(label_style))
        .child(Text::new(" ").style(label_style));

    let frame = Frame::new()
        .title(full_label)
        .border(true)
        .border_style(border_style)
        .title_style(label_style)
        .inner_style(Style::default().bg(frame_bg))
        .width(Length::Px(11))
        .height(Length::Px(5))
        .child(inner);

    MouseRegion::new()
        .on_mouse_down(ctx.link().callback(move |_| Msg::Hit(cell_idx)))
        .hover_effect(VisualEffect::transform_fg(ColorTransform::Lighten(0.3)))
        .child(frame)
        .into()
}

fn main() -> Result<()> {
    App::new().title("Whack-a-mole").mount(WhackAMole).run()
}
