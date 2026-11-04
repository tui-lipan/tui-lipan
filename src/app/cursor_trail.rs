//! A short smear from where the host caret was to where it jumped.
//!
//! The runner watches the caret every presented frame takes to the host, whichever render path
//! drew it. When the caret leaves one widget for another (or, with [`CursorTrailTrigger::Jump`],
//! moves far enough in one step), a trail is started from the old cell to the new one. Its tail
//! eases in toward the caret over [`CursorTrail::duration`], and it is painted as the last pass of
//! an ordinary full draw: tinting cell backgrounds only, so the text underneath stays readable and
//! the next full draw's diff puts every cell back without any bookkeeping of its own.
//!
//! The fast paths (row damage, spinner damage, incremental scroll) patch the host from a retained
//! frame and never run that pass, so the runner routes every frame through the full draw while a
//! trail is running and for the one frame that clears it.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::Color as RColor;
use web_time::Instant;

use crate::core::node::NodeId;
use crate::style::Color;

/// What makes a [`CursorTrail`] appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorTrailTrigger {
    /// Only when focus moves to another widget and the caret moves with it.
    #[default]
    FocusChange,
    /// On a focus change, and whenever the caret moves at least `columns` columns or `rows` rows
    /// in one step inside the same widget.
    Jump {
        /// The fewest columns a same-widget move must span.
        columns: u16,
        /// The fewest rows a same-widget move must span.
        rows: u16,
    },
}

impl CursorTrailTrigger {
    /// [`Self::Jump`] with thresholds that skip typing and moves to a neighbouring line.
    pub const fn jump() -> Self {
        Self::Jump {
            columns: 8,
            rows: 2,
        }
    }
}

/// A trail drawn behind the host caret when it jumps, set with
/// [`App::cursor_trail`](crate::App::cursor_trail) or
/// [`Context::set_cursor_trail`](crate::Context::set_cursor_trail).
///
/// The trail runs from the cell the caret left to the cell it landed on, brightest at the caret,
/// and shrinks into it. It tints cell backgrounds and leaves text alone. Blending needs RGB: over a
/// background in a palette color the trail shows as a solid band on its brighter half instead of a
/// gradient. Inline surfaces never draw one.
///
/// ```
/// use std::time::Duration;
/// use tui_lipan::prelude::*;
///
/// let trail = CursorTrail::new()
///     .trigger(CursorTrailTrigger::jump())
///     .duration(Duration::from_millis(200))
///     .color(Color::Rgb(137, 180, 250));
/// assert!(trail.is_visible());
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorTrail {
    /// What makes the trail appear.
    pub trigger: CursorTrailTrigger,
    /// How long a trail takes to shrink into the caret. Zero turns trails off.
    pub duration: Duration,
    /// The trail's color. `None` uses the focused widget's caret color, then the host terminal's
    /// foreground, then the color of the text under the caret.
    pub color: Option<Color>,
}

impl CursorTrail {
    /// The duration a [`CursorTrail::new`] trail runs for.
    pub const DEFAULT_DURATION: Duration = Duration::from_millis(160);

    /// A trail on focus changes, over [`Self::DEFAULT_DURATION`], in the caret's own color.
    pub const fn new() -> Self {
        Self {
            trigger: CursorTrailTrigger::FocusChange,
            duration: Self::DEFAULT_DURATION,
            color: None,
        }
    }

    /// Set what makes the trail appear.
    #[must_use]
    pub const fn trigger(mut self, trigger: CursorTrailTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Set how long a trail takes to shrink into the caret.
    #[must_use]
    pub const fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set the trail's color.
    #[must_use]
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Whether this configuration ever draws a trail.
    pub fn is_visible(&self) -> bool {
        !self.duration.is_zero()
    }

    fn triggers(&self, from: Caret, to: Caret) -> bool {
        if from.position == to.position {
            return false;
        }
        if from.owner != to.owner {
            return true;
        }
        match self.trigger {
            CursorTrailTrigger::FocusChange => false,
            CursorTrailTrigger::Jump { columns, rows } => {
                let dx = from.position.x.abs_diff(to.position.x);
                let dy = from.position.y.abs_diff(to.position.y);
                (columns > 0 && dx >= columns) || (rows > 0 && dy >= rows)
            }
        }
    }
}

impl Default for CursorTrail {
    fn default() -> Self {
        Self::new()
    }
}

/// The host caret as one presented frame left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Caret {
    position: Position,
    owner: Option<NodeId>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveTrail {
    /// Where the tail starts, in cell coordinates. Fractional when a trail restarts mid-flight.
    from: (f32, f32),
    to: Position,
    started: Instant,
    duration: Duration,
    color: Option<Color>,
}

impl ActiveTrail {
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    fn tail(&self, progress: f32) -> (f32, f32) {
        let eased = ease_out_cubic(progress);
        let (fx, fy) = self.from;
        let (tx, ty) = (f32::from(self.to.x), f32::from(self.to.y));
        (fx + (tx - fx) * eased, fy + (ty - fy) * eased)
    }
}

/// What the runner should do about the trail on this pass of the event loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CursorTrailTick {
    /// A frame is due: the trail moved on, or it has ended and its cells must be cleared.
    pub(crate) needs_paint: bool,
    /// How long until the next trail frame is due.
    pub(crate) next_due: Option<Duration>,
}

/// One frame of a running trail, ready to paint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorTrailFrame {
    tail: (f32, f32),
    head: Position,
    /// Overall strength, fading from one to nothing as the trail runs.
    fade: f32,
    color: Option<Color>,
}

/// The runner's trail bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct CursorTrailState {
    /// The last caret a frame showed. Kept while the caret is hidden, so a caret that disappears
    /// (a pending chord, a redraw) and comes back somewhere else still trails from where it was.
    last: Option<Caret>,
    active: Option<ActiveTrail>,
    /// The trail the last full draw painted, which the host still shows.
    painted: Option<CursorTrailFrame>,
    last_frame: Option<Instant>,
}

impl CursorTrailState {
    /// Record where a presented frame left the caret, starting a trail when it jumped.
    pub(crate) fn observe(
        &mut self,
        config: Option<CursorTrail>,
        position: Option<Position>,
        owner: Option<NodeId>,
        caret_color: Option<Color>,
        now: Instant,
    ) {
        let Some(position) = position else {
            return;
        };
        let caret = Caret { position, owner };
        let Some(last) = self.last.replace(caret) else {
            return;
        };
        let Some(config) = config.filter(CursorTrail::is_visible) else {
            self.active = None;
            return;
        };
        if config.triggers(last, caret) {
            // A jump during a running trail starts the new one from the old one's tail, so the
            // smear never snaps back to a cell the eye has already left.
            let from = self
                .active
                .filter(|active| active.progress(now) < 1.0)
                .map(|active| active.tail(active.progress(now)))
                .unwrap_or((f32::from(last.position.x), f32::from(last.position.y)));
            self.active = Some(ActiveTrail {
                from,
                to: position,
                started: now,
                duration: config.duration,
                color: config.color.or(caret_color),
            });
            self.last_frame = None;
        } else if let Some(active) = self.active.as_mut() {
            // Small moves while the trail runs carry its head along with the caret.
            active.to = position;
        }
    }

    /// Advance the trail clock: end a finished trail and say when the next frame is due.
    pub(crate) fn tick(
        &mut self,
        config: Option<CursorTrail>,
        frame_interval: Duration,
        now: Instant,
    ) -> CursorTrailTick {
        if config.is_none_or(|config| !config.is_visible()) {
            self.active = None;
        }
        if self
            .active
            .is_some_and(|active| active.progress(now) >= 1.0)
        {
            self.active = None;
        }
        if self.active.is_some() {
            let since = self
                .last_frame
                .map(|last| now.saturating_duration_since(last));
            let due = since.is_none_or(|since| since >= frame_interval);
            return CursorTrailTick {
                needs_paint: due,
                next_due: Some(match since {
                    Some(since) if !due => frame_interval - since,
                    _ => frame_interval,
                }),
            };
        }
        CursorTrailTick {
            needs_paint: self.painted.is_some(),
            next_due: None,
        }
    }

    /// Whether this frame must be a full draw: a trail is running, or the host still shows one.
    pub(crate) fn requires_full_draw(&self) -> bool {
        self.active.is_some() || self.painted.is_some()
    }

    /// The trail to paint over a full draw at `now`, if one is running.
    pub(crate) fn frame(&self, now: Instant) -> Option<CursorTrailFrame> {
        let active = self.active?;
        let progress = active.progress(now);
        if progress >= 1.0 {
            return None;
        }
        Some(CursorTrailFrame {
            tail: active.tail(progress),
            head: active.to,
            fade: 1.0 - progress * progress,
            color: active.color,
        })
    }

    /// Record that a full draw went out, with the trail it painted.
    pub(crate) fn record_full_draw(&mut self, painted: Option<CursorTrailFrame>, now: Instant) {
        self.painted = painted;
        if painted.is_some() {
            self.last_frame = Some(now);
        }
    }

    /// The trail the host shows from the last full draw, for a capture of that frame.
    pub(crate) fn painted(&self) -> Option<CursorTrailFrame> {
        self.painted
    }
}

/// Strongest tint, right behind the caret.
const HEAD_ALPHA: f32 = 0.8;
/// Tint at the far end of the tail.
const TAIL_ALPHA: f32 = 0.12;
/// Without RGB to blend, cells at least this strong take the trail color outright.
const SOLID_THRESHOLD: f32 = 0.45;

impl CursorTrailFrame {
    /// Tint the cells between the tail and the caret. The caret's own cell is left alone.
    ///
    /// `host` is the host terminal's (foreground, background), when it reported them: the
    /// background resolves cells left at the terminal default, the foreground is the last color
    /// fallback before the text under the caret.
    pub(crate) fn paint(&self, buffer: &mut Buffer, host: Option<(Color, Color)>) {
        let color = self
            .color
            .or(host.map(|(fg, _)| fg))
            .or_else(|| {
                buffer
                    .cell(self.head)
                    .and_then(|cell| from_ratatui_color(cell.fg))
            })
            .unwrap_or(Color::Gray);
        let trail_rgb = match color {
            Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        };
        let solid = crate::backend::ratatui_backend::common::to_ratatui_color(color);
        let host_bg = host.and_then(|(_, bg)| match bg {
            Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        });

        let (tx, ty) = self.tail;
        let (hx, hy) = (f32::from(self.head.x), f32::from(self.head.y));
        let steps = (hx - tx).abs().max((hy - ty).abs()).ceil() as u32;
        if steps == 0 {
            return;
        }
        let area = buffer.area;
        let mut previous: Option<Position> = None;
        for step in 0..steps {
            let along = step as f32 / steps as f32;
            let x = (tx + (hx - tx) * along).round();
            let y = (ty + (hy - ty) * along).round();
            if x < 0.0 || y < 0.0 {
                continue;
            }
            let position = Position::new(x as u16, y as u16);
            if position == self.head || previous == Some(position) || !area.contains(position) {
                continue;
            }
            previous = Some(position);
            let alpha = (TAIL_ALPHA + (HEAD_ALPHA - TAIL_ALPHA) * along * along) * self.fade;
            let Some(cell) = buffer.cell_mut(position) else {
                continue;
            };
            let under = match cell.bg {
                RColor::Rgb(r, g, b) => Some((r, g, b)),
                RColor::Reset => host_bg,
                _ => None,
            };
            match (trail_rgb, under) {
                (Some(trail), Some(under)) => {
                    let (r, g, b) = mix(under, trail, alpha);
                    cell.bg = RColor::Rgb(r, g, b);
                }
                _ if alpha >= SOLID_THRESHOLD => cell.bg = solid,
                _ => {}
            }
        }
    }
}

fn from_ratatui_color(color: RColor) -> Option<Color> {
    match color {
        RColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
        _ => None,
    }
}

fn mix(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    let channel = |a: u8, b: u8| {
        (f32::from(a) + (f32::from(b) - f32::from(a)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

fn ease_out_cubic(t: f32) -> f32 {
    let inverse = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inverse * inverse * inverse
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    const RED: (u8, u8, u8) = (255, 0, 0);

    fn node(id: u32) -> Option<NodeId> {
        Some(NodeId::new(id, 0))
    }

    fn trail() -> Option<CursorTrail> {
        Some(CursorTrail::new().color(Color::Rgb(RED.0, RED.1, RED.2)))
    }

    fn running(state: &mut CursorTrailState, start: Instant) {
        state.observe(trail(), Some(Position::new(1, 1)), node(1), None, start);
        state.observe(trail(), Some(Position::new(11, 1)), node(2), None, start);
    }

    #[test]
    fn a_focus_change_starts_a_trail_and_a_same_widget_move_does_not() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        state.observe(trail(), Some(Position::new(1, 1)), node(1), None, start);
        state.observe(trail(), Some(Position::new(30, 9)), node(1), None, start);
        assert!(state.frame(start).is_none(), "a move inside one widget");
        state.observe(trail(), Some(Position::new(2, 2)), node(2), None, start);
        assert!(state.frame(start).is_some(), "a move to another widget");
    }

    #[test]
    fn jump_thresholds_gate_same_widget_moves() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::Jump {
            columns: 8,
            rows: 2,
        }));
        let mut state = CursorTrailState::default();
        state.observe(jump, Some(Position::new(0, 0)), node(1), None, start);
        state.observe(jump, Some(Position::new(7, 1)), node(1), None, start);
        assert!(state.frame(start).is_none(), "7 columns and 1 row");
        state.observe(jump, Some(Position::new(7, 3)), node(1), None, start);
        assert!(state.frame(start).is_some(), "2 rows");
    }

    #[test]
    fn a_hidden_caret_keeps_its_last_cell_for_the_next_trail() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        state.observe(trail(), Some(Position::new(1, 1)), node(1), None, start);
        state.observe(trail(), None, node(1), None, start);
        state.observe(trail(), Some(Position::new(9, 1)), node(2), None, start);
        let frame = state.frame(start).expect("trail after the caret came back");
        assert_eq!(frame.tail, (1.0, 1.0));
    }

    #[test]
    fn the_trail_ends_after_its_duration_and_asks_for_one_clearing_frame() {
        let start = Instant::now();
        let interval = Duration::from_millis(16);
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        assert!(state.tick(trail(), interval, start).needs_paint);
        state.record_full_draw(state.frame(start), start);
        assert!(state.requires_full_draw());

        let later = start + interval / 2;
        let tick = state.tick(trail(), interval, later);
        assert!(!tick.needs_paint, "not due yet");
        assert_eq!(tick.next_due, Some(interval / 2));

        let done = start + CursorTrail::DEFAULT_DURATION;
        let tick = state.tick(trail(), interval, done);
        assert!(tick.needs_paint, "one frame clears the trail");
        assert!(state.frame(done).is_none());
        state.record_full_draw(None, done);
        assert!(!state.requires_full_draw());
        assert!(!state.tick(trail(), interval, done).needs_paint);
    }

    #[test]
    fn disabling_the_trail_stops_a_running_one() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        state.tick(None, Duration::from_millis(16), start);
        assert!(state.frame(start).is_none());
        let zero = Some(CursorTrail::new().duration(Duration::ZERO));
        running(&mut state, start);
        state.tick(zero, Duration::from_millis(16), start);
        assert!(state.frame(start).is_none());
    }

    #[test]
    fn painting_tints_backgrounds_toward_the_caret_and_spares_the_caret_cell() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        let frame = state.frame(start).expect("running trail");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        for cell in buffer.content.iter_mut() {
            cell.bg = RColor::Rgb(0, 0, 0);
            cell.set_symbol("x");
        }
        frame.paint(&mut buffer, None);

        let red = |x: u16| match buffer[(x, 1)].bg {
            RColor::Rgb(r, _, _) => r,
            other => panic!("cell {x} lost its RGB background: {other:?}"),
        };
        assert!(
            red(2) < red(6) && red(6) < red(10),
            "brightest next to the caret"
        );
        assert_eq!(buffer[(11, 1)].bg, RColor::Rgb(0, 0, 0), "the caret cell");
        assert_eq!(buffer[(12, 1)].bg, RColor::Rgb(0, 0, 0), "past the caret");
        assert_eq!(buffer[(5, 0)].bg, RColor::Rgb(0, 0, 0), "off the path");
        assert_eq!(buffer[(5, 1)].symbol(), "x", "text stays");
    }

    #[test]
    fn default_backgrounds_blend_against_the_host_background_or_go_solid() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        let frame = state.frame(start).expect("running trail");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        frame.paint(
            &mut buffer,
            Some((Color::Rgb(200, 200, 200), Color::Rgb(0, 0, 40))),
        );
        assert!(matches!(buffer[(10, 1)].bg, RColor::Rgb(_, 0, _)));

        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        frame.paint(&mut buffer, None);
        assert_eq!(buffer[(10, 1)].bg, RColor::Rgb(RED.0, RED.1, RED.2));
        assert_eq!(buffer[(2, 1)].bg, RColor::Reset, "the faint end stays off");
    }

    #[test]
    fn the_tail_eases_into_the_caret() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        let early = state.frame(start).expect("running").tail.0;
        let late = state
            .frame(start + CursorTrail::DEFAULT_DURATION / 2)
            .expect("running")
            .tail
            .0;
        assert_eq!(early, 1.0);
        assert!(late > 8.0 && late < 11.0, "tail at {late}");
    }
}
