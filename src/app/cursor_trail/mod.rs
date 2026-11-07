//! A smear that follows the host caret when it jumps.
//!
//! The runner watches the caret every presented frame takes to the host, whichever render path
//! drew it. When the caret leaves one widget for another (or, with [`CursorTrailTrigger::Jump`],
//! moves far enough in one step), the caret's rectangle is stretched from the old cell to the new
//! one and pulled back into the caret, by spring physics ([`CursorTrailMotion::Spring`]) or an
//! eased glide ([`CursorTrailMotion::Glide`]). Sparks can fly off it ([`CursorTrailParticles`]).
//!
//! The smear is painted as the last pass of an ordinary full draw, so the next full draw's diff
//! puts every cell back without any bookkeeping of its own. The fast paths (row damage, spinner
//! damage, incremental scroll) patch the host from a retained frame and never run that pass, so
//! the runner routes every frame through the full draw while a smear or a spark is on screen and
//! for the one frame that clears it.

mod legacy_glyphs;
mod motion;
mod particles;
mod raster;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use web_time::Instant;

use crate::CaretShape;
use crate::core::node::NodeId;
use crate::style::{Color, HostTerminalColors};

use motion::{Corners, Smear, caret_rect};
use particles::{Spark, Sparks};
use raster::{GlyphSet, Hull, SmearPaint};

/// What makes a [`CursorTrail`] appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorTrailTrigger {
    /// Only when focus moves to another widget and the caret moves with it.
    #[default]
    FocusChange,
    /// On a focus change, and whenever the caret moves at least `columns` columns or `rows` rows
    /// in one step inside the same widget. The move is measured within the widget, so a widget
    /// that moves (an animated layout, a drag) carries its caret without starting a trail.
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

/// The springs that pull the smear into the caret: its head after the caret, and its tail after
/// the head along the path the head took.
///
/// Every value is applied once per sixtieth of a second, whatever the frame rate. `stiffness`,
/// `trailing_stiffness`, and `damping` work as smear-cursor.nvim's settings of the same name do
/// for its leading and trailing corners, so a spring tuned for it feels the same here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorTrailSpring {
    /// How hard the head is pulled toward the caret, from 0 (not at all) to 1.
    pub stiffness: f32,
    /// How hard the tail is pulled after the head. Lower than `stiffness` stretches the smear.
    pub trailing_stiffness: f32,
    /// How full the smear stays toward its tail: its tail is `e / (1 + e)` of the caret's size,
    /// so `0.3` tapers to about a quarter and `3.0` keeps three quarters. The width eases out
    /// toward that, so the smear stays full near the caret and thins mostly at its end.
    pub trailing_exponent: f32,
    /// How much velocity is lost each step, from 0 (bouncy) to 1 (no momentum).
    pub damping: f32,
    /// The smear stops once its head and tail are this close to the caret, in cells.
    pub stop_distance: f32,
}

impl CursorTrailSpring {
    /// The default spring: the head lands on the caret in about three steps while the tail
    /// closes in over about six, so the smear visibly stretches and shrinks rather than flashing
    /// past, and is gone in about a quarter of a second. It tapers to a quarter of the caret at
    /// its tail, so a diagonal smear is a streak rather than a band the caret's full height.
    pub const DEFAULT: Self = Self {
        stiffness: 0.5,
        trailing_stiffness: 0.28,
        trailing_exponent: 0.3,
        damping: 0.82,
        stop_distance: 0.1,
    };

    /// smear-cursor.nvim's own defaults: a quicker, more compact smear that is over in about
    /// five steps and keeps most of the caret's size to its tail.
    pub const SMEAR_CURSOR: Self = Self {
        stiffness: 0.6,
        trailing_stiffness: 0.45,
        trailing_exponent: 3.0,
        damping: 0.85,
        stop_distance: 0.1,
    };
}

impl Default for CursorTrailSpring {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How a [`CursorTrail`] moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorTrailMotion {
    /// The head is pulled after the caret by a damped spring, and the tail after the head along
    /// the path it took: the smear stretches out behind a jump and snaps into the caret, and a
    /// jump mid-flight swings the head round with its momentum, the smear bending after it.
    Spring(CursorTrailSpring),
    /// The leading edge sits on the caret at once and the trailing edge eases into it over
    /// `duration`.
    Glide {
        /// How long the trailing edge takes. Zero turns trails off.
        duration: Duration,
    },
}

impl Default for CursorTrailMotion {
    fn default() -> Self {
        Self::Spring(CursorTrailSpring::DEFAULT)
    }
}

/// How a [`CursorTrail`] is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorTrailFill {
    /// In the trail color, anti-aliased with the glyphs [`CursorTrailGlyphs`] allows. Cells the
    /// smear covers evenly keep their text in inverse colors. The smear draws the caret itself
    /// while it moves, so the host caret is hidden until it settles.
    #[default]
    Solid,
    /// As a fainter tint that leaves every character and the host caret in place: cells with
    /// text take it in their background, and blank cells are drawn anti-aliased like
    /// [`Solid`](Self::Solid).
    Tint,
}

/// Which glyphs a [`CursorTrail`] draws its edges with.
///
/// Richer sets follow a diagonal more closely, but only look right where the terminal draws the
/// glyphs itself or the font has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorTrailGlyphs {
    /// The richest set the host terminal is known to draw itself: octants on Ghostty, sextants on
    /// Kitty, WezTerm, and foot, block elements anywhere else.
    #[default]
    Auto,
    /// Block Elements (U+2580): halves, eighths, and quadrants. Every terminal draws them.
    Blocks,
    /// Also the sextants, diagonal wedges, and triangles of Symbols for Legacy Computing
    /// (U+1FB00, Unicode 13).
    Sextants,
    /// Also its octants (U+1CD00, Unicode 16) and the half-cell triangles (U+25E2).
    Octants,
}

impl CursorTrailGlyphs {
    fn resolve(self) -> GlyphSet {
        match self {
            Self::Blocks => GlyphSet::Blocks,
            Self::Sextants => GlyphSet::Sextants,
            Self::Octants => GlyphSet::Octants,
            Self::Auto => {
                static DETECTED: std::sync::OnceLock<GlyphSet> = std::sync::OnceLock::new();
                *DETECTED.get_or_init(|| detect_glyphs(|name| std::env::var(name).ok()))
            }
        }
    }
}

/// The richest glyph set the host terminal draws itself, judged from its environment.
fn detect_glyphs(var: impl Fn(&str) -> Option<String>) -> GlyphSet {
    let term = var("TERM").unwrap_or_default().to_ascii_lowercase();
    let program = var("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
    let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    if program == "ghostty" || term.contains("ghostty") || set("GHOSTTY_RESOURCES_DIR") {
        GlyphSet::Octants
    } else if term.contains("kitty")
        || set("KITTY_WINDOW_ID")
        || program == "wezterm"
        || set("WEZTERM_PANE")
        || term.starts_with("foot")
    {
        GlyphSet::Sextants
    } else {
        GlyphSet::Blocks
    }
}

/// Sparks thrown off by a moving smear, drawn as braille dots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorTrailParticles {
    /// Sparks per cell the smear travels.
    pub per_cell: f32,
    /// The most sparks alive at once.
    pub max: u16,
    /// How long a spark lives, at most; each lives between 60% and all of it.
    pub lifetime: Duration,
    /// How fast a spark flies off in a random direction, at most, in cells per second.
    pub spread: f32,
    /// How much of the smear's own velocity a spark keeps, from 0 to 1.
    pub inherit: f32,
    /// Downward acceleration, in cells per second squared. Negative values rise.
    pub gravity: f32,
    /// How quickly a spark slows, per second.
    pub drag: f32,
}

impl Default for CursorTrailParticles {
    fn default() -> Self {
        Self {
            per_cell: 1.5,
            max: 100,
            lifetime: Duration::from_millis(300),
            spread: 6.0,
            inherit: 0.25,
            gravity: 20.0,
            drag: 3.0,
        }
    }
}

/// A smear that follows the host caret when it jumps, set with
/// [`App::cursor_trail`](crate::App::cursor_trail) or
/// [`Context::set_cursor_trail`](crate::Context::set_cursor_trail).
///
/// The smear is the caret's own shape (block, bar, or underline) stretched from the cell the caret
/// left to the cell it landed on, and pulled back into the caret. Inline surfaces never draw one.
///
/// ```
/// use std::time::Duration;
/// use tui_lipan::prelude::*;
///
/// let trail = CursorTrail::new()
///     .trigger(CursorTrailTrigger::jump())
///     .motion(CursorTrailMotion::Spring(CursorTrailSpring {
///         damping: 0.7,
///         ..CursorTrailSpring::DEFAULT
///     }))
///     .fill(CursorTrailFill::Tint)
///     .particles(Some(CursorTrailParticles::default()))
///     .color(Color::Rgb(137, 180, 250));
/// assert!(trail.is_visible());
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorTrail {
    /// What makes the trail appear.
    pub trigger: CursorTrailTrigger,
    /// How the smear moves.
    pub motion: CursorTrailMotion,
    /// How the smear is drawn.
    pub fill: CursorTrailFill,
    /// The trail's color. `None` uses the focused widget's caret color, then the host terminal's
    /// cursor color, then its foreground, then the color of the text under the caret.
    pub color: Option<Color>,
    /// The smear's strength at the caret, from 0 (invisible) to 1.
    pub opacity: f32,
    /// How much the smear fades toward its tail, from 0 (evenly) to 1 (to nothing).
    pub taper: f32,
    /// The smear's tail trails the caret by no more than this along its way, from the very first
    /// frame, in cell widths measured on screen (a row counts for as many as a cell is taller
    /// than wide). Zero or less (the default) leaves it unbounded.
    pub max_length: f32,
    /// Sparks thrown off the smear, or `None` (the default) for none.
    pub particles: Option<CursorTrailParticles>,
    /// Which glyphs the smear's edges are drawn with.
    pub glyphs: CursorTrailGlyphs,
}

impl CursorTrail {
    /// The duration of [`CursorTrail::glide`].
    pub const DEFAULT_DURATION: Duration = Duration::from_millis(160);
    /// The default [`Self::max_length`]: unbounded, so a long jump's streak starts at the cell
    /// the caret left and shrinks smoothly, rather than having its tail pulled forward.
    pub const DEFAULT_MAX_LENGTH: f32 = 0.0;

    /// A solid, spring-driven smear on focus changes, in the caret's own color.
    pub const fn new() -> Self {
        Self {
            trigger: CursorTrailTrigger::FocusChange,
            motion: CursorTrailMotion::Spring(CursorTrailSpring::DEFAULT),
            fill: CursorTrailFill::Solid,
            color: None,
            opacity: 1.0,
            taper: 0.0,
            max_length: Self::DEFAULT_MAX_LENGTH,
            particles: None,
            glyphs: CursorTrailGlyphs::Auto,
        }
    }

    /// [`CursorTrailMotion::Glide`] over [`Self::DEFAULT_DURATION`].
    pub const fn glide() -> CursorTrailMotion {
        CursorTrailMotion::Glide {
            duration: Self::DEFAULT_DURATION,
        }
    }

    /// Set what makes the trail appear.
    #[must_use]
    pub const fn trigger(mut self, trigger: CursorTrailTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Set how the smear moves.
    #[must_use]
    pub const fn motion(mut self, motion: CursorTrailMotion) -> Self {
        self.motion = motion;
        self
    }

    /// Set how the smear is drawn.
    #[must_use]
    pub const fn fill(mut self, fill: CursorTrailFill) -> Self {
        self.fill = fill;
        self
    }

    /// Set the trail's color.
    #[must_use]
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the smear's strength at the caret.
    #[must_use]
    pub const fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Set how much the smear fades toward its tail.
    #[must_use]
    pub const fn taper(mut self, taper: f32) -> Self {
        self.taper = taper;
        self
    }

    /// Set the longest the smear may stretch, in cell widths measured on screen.
    #[must_use]
    pub const fn max_length(mut self, cells: f32) -> Self {
        self.max_length = cells;
        self
    }

    /// Set the sparks, or `None` for none.
    #[must_use]
    pub const fn particles(mut self, particles: Option<CursorTrailParticles>) -> Self {
        self.particles = particles;
        self
    }

    /// Set which glyphs the smear's edges are drawn with.
    #[must_use]
    pub const fn glyphs(mut self, glyphs: CursorTrailGlyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Whether this configuration ever draws a trail.
    pub fn is_visible(&self) -> bool {
        self.opacity > 0.0
            && !matches!(self.motion, CursorTrailMotion::Glide { duration } if duration.is_zero())
    }

    /// Whether the caret's move from `from` to `to` starts a trail. Only the part of the move
    /// made inside its widget counts, not the part its widget carried it.
    fn triggers(&self, from: Caret, to: Caret) -> bool {
        if from.owner_id() != to.owner_id() {
            return from.position != to.position;
        }
        let (carried_x, carried_y) = to.carried_from(from);
        let dx = (i32::from(to.position.x) - i32::from(from.position.x) - carried_x).unsigned_abs();
        let dy = (i32::from(to.position.y) - i32::from(from.position.y) - carried_y).unsigned_abs();
        if dx == 0 && dy == 0 {
            return false;
        }
        match self.trigger {
            CursorTrailTrigger::FocusChange => false,
            CursorTrailTrigger::Jump { columns, rows } => {
                (columns > 0 && dx >= u32::from(columns)) || (rows > 0 && dy >= u32::from(rows))
            }
        }
    }
}

impl Default for CursorTrail {
    fn default() -> Self {
        Self::new()
    }
}

/// A point or offset in cell coordinates: x to the right, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vec2 {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Vec2 {
    pub(crate) const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub(crate) const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub(crate) fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    pub(crate) fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub(crate) fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    pub(crate) fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub(crate) fn lerp(self, other: Self, amount: f32) -> Self {
        self + (other - self).scale(amount)
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    let inverse = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inverse * inverse * inverse
}

/// The widget that owns the caret, and where its layout rectangle sits on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretOwner {
    pub(crate) id: NodeId,
    pub(crate) origin: (i32, i32),
}

/// The host caret as one presented frame left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Caret {
    position: Position,
    owner: Option<CaretOwner>,
    shape: CaretShape,
}

impl Caret {
    fn rect(self) -> Corners {
        caret_rect(self.position.x, self.position.y, self.shape)
    }

    fn owner_id(self) -> Option<NodeId> {
        self.owner.map(|owner| owner.id)
    }

    /// How far the widget that owns the caret moved since `earlier`, in cells, when it is the
    /// same widget: a caret carried along by its widget (a slide, a drag) has not jumped.
    fn carried_from(self, earlier: Self) -> (i32, i32) {
        match (earlier.owner, self.owner) {
            (Some(from), Some(to)) if from.id == to.id => {
                (to.origin.0 - from.origin.0, to.origin.1 - from.origin.1)
            }
            _ => (0, 0),
        }
    }
}

fn carry_offset((dx, dy): (i32, i32)) -> Vec2 {
    Vec2::new(dx as f32, dy as f32)
}

fn carry_corners(corners: Corners, offset: Vec2) -> Corners {
    corners.map(|corner| corner + offset)
}

/// The focused widget's caret, as the runner hands it to [`CursorTrailState::observe`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CaretLook {
    pub(crate) shape: CaretShape,
    pub(crate) color: Option<Color>,
}

/// A jump inside one widget, waiting for the caret to come to rest before its smear starts.
#[derive(Clone, Copy, Debug)]
struct Landing {
    from: Corners,
    to: Corners,
    since: Instant,
    last_move: Instant,
    color: Option<Color>,
    /// A solid trail holds the caret where it was until it starts, rather than let the host
    /// show it at its destination first and the smear then start from behind it.
    hold: bool,
}

#[derive(Clone, Debug)]
struct Running {
    smear: Smear,
    /// How far along its path the smear's head was in the last drawn frame. A smear whose tail
    /// has passed that, however many physics steps a slow frame let pass, reaches back to it, so
    /// the streak never breaks.
    drawn: Option<f32>,
    color: Option<Color>,
    /// The smear draws the caret itself, so the host caret stays hidden.
    hides: bool,
}

/// What the runner should do about the trail on this pass of the event loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CursorTrailTick {
    /// A frame is due: the trail moved on, or it has ended and its cells must be cleared.
    pub(crate) needs_paint: bool,
    /// How long until the next trail frame is due.
    pub(crate) next_due: Option<Duration>,
}

/// One frame of the trail, ready to paint.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CursorTrailFrame {
    smear: Option<SmearShape>,
    sparks: Vec<Spark>,
    /// The caret's cell, which sparks keep clear of so they never draw over the caret.
    caret: Option<Position>,
    fill: CursorTrailFill,
    color: Option<Color>,
    opacity: f32,
    taper: f32,
    glyphs: GlyphSet,
}

#[derive(Clone, Debug, PartialEq)]
struct SmearShape {
    /// The smear as convex pieces along its path, each at most the caret's width.
    pieces: Vec<Hull>,
    /// How far each piece is from the tail (0) to the head (1).
    along: Vec<f32>,
    head: Vec2,
    tail: Vec2,
    /// How far along its path the head is, for the next frame to reach back to.
    head_arc: f32,
    target: Position,
}

/// The runner's trail bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct CursorTrailState {
    /// The last caret a frame showed. Kept while the caret is hidden, so a caret that disappears
    /// (a pending chord, a redraw) and comes back somewhere else still trails from where it was.
    last: Option<Caret>,
    landing: Option<Landing>,
    running: Option<Running>,
    sparks: Sparks,
    /// Spark time integrated but not yet a whole step.
    spark_pending: Duration,
    /// When the trail was last advanced.
    clock: Option<Instant>,
    /// The trail the last full draw painted, which the host still shows.
    painted: Option<CursorTrailFrame>,
    last_frame: Option<Instant>,
    /// The host's cell height over its width, when known, so the smear measures on screen.
    cell_aspect: Option<f32>,
    /// A skipped move is pending: the next caret move that would start a trail starts none.
    skip: bool,
}

/// How soon after a jump inside one widget the caret must hold still before the smear starts:
/// longer than the gap between a shell's newline and its prompt.
const LANDING_QUIET: Duration = Duration::from_millis(50);
/// The longest a jump waits for a caret that keeps moving before its smear starts anyway.
const LANDING_MAX: Duration = Duration::from_millis(200);

impl CursorTrailState {
    /// Record where a presented frame left the caret, starting a trail when it jumped.
    pub(crate) fn observe(
        &mut self,
        config: Option<CursorTrail>,
        position: Option<Position>,
        owner: Option<CaretOwner>,
        look: CaretLook,
        now: Instant,
    ) {
        let Some(position) = position else {
            return;
        };
        let caret = Caret {
            position,
            owner,
            shape: look.shape,
        };
        let Some(last) = self.last.replace(caret) else {
            return;
        };
        let Some(config) = config.filter(CursorTrail::is_visible) else {
            self.stop();
            return;
        };
        // A widget that moves (a slide, a drag) carries its caret, and a trail already on its way
        // with it: only what the caret does inside its widget is a jump.
        let carried = carry_offset(caret.carried_from(last));
        if carried != Vec2::ZERO {
            self.carry(carried);
        }
        if self.skip {
            // Nothing starts while a skip is pending, so there is nothing to move on either.
            if config.triggers(last, caret) {
                self.skip = false;
            }
            return;
        }
        let target = caret.rect();
        // The trail takes the color of the caret it is following now, so one sent on to another
        // widget mid-flight does not keep the color of the widget it passed through.
        let color = config.color.or(look.color);
        if let Some(running) = self.running.as_mut() {
            // A smear in flight follows the caret wherever it goes next, keeping its momentum.
            running.smear.retarget(target);
            running.color = color;
            return;
        }
        if let Some(landing) = self.landing.as_mut() {
            // A caret that lands in steps (a shell's newline, then its prompt, in separate
            // writes) is followed to where it comes to rest, and the smear starts once, straight
            // from where it was: never through a cell it held for a frame.
            landing.color = color;
            if landing.to != target {
                landing.to = target;
                landing.last_move = now;
            }
            if last.owner_id() != caret.owner_id() {
                self.launch(now, config.max_length);
            }
            return;
        }
        if config.triggers(last, caret) {
            self.landing = Some(Landing {
                from: carry_corners(last.rect(), carried),
                to: target,
                since: now,
                last_move: now,
                color,
                hold: config.fill == CursorTrailFill::Solid,
            });
            // A focus change lands in one step and starts at once; a jump inside one widget may be
            // the first of several writes, so it waits for the caret to settle.
            if last.owner_id() != caret.owner_id() {
                self.launch(now, config.max_length);
            }
        }
    }

    fn launch(&mut self, now: Instant, max_length: f32) {
        if let Some(landing) = self.landing.take() {
            self.running = Some(Running {
                smear: Smear::new(
                    landing.from,
                    landing.to,
                    max_length,
                    self.cell_aspect.unwrap_or(motion::DEFAULT_CELL_ASPECT),
                ),
                drawn: None,
                color: landing.color,
                hides: landing.hold,
            });
            self.clock = Some(now);
            self.last_frame = None;
        }
    }

    /// Move everything the trail is drawing, or waiting to draw, by `offset` cells.
    fn carry(&mut self, offset: Vec2) {
        if let Some(landing) = self.landing.as_mut() {
            landing.from = carry_corners(landing.from, offset);
            landing.to = carry_corners(landing.to, offset);
        }
        if let Some(running) = self.running.as_mut() {
            running.smear.carry(offset);
        }
        self.sparks.carry(offset);
    }

    /// Stop the trail now and let the caret's next move that would start one start none. Frames
    /// that leave the caret where it was, or carry it with its widget, keep the skip pending.
    pub(crate) fn skip_next_move(&mut self) {
        self.stop();
        self.skip = true;
    }

    fn stop(&mut self) {
        self.landing = None;
        self.running = None;
        self.sparks.clear();
        self.spark_pending = Duration::ZERO;
    }

    /// Integrate the trail up to `now`: start a landed jump, move the smear, move the sparks.
    pub(crate) fn advance(&mut self, config: Option<CursorTrail>, now: Instant) {
        let Some(config) = config.filter(CursorTrail::is_visible) else {
            self.stop();
            return;
        };
        if let Some(landing) = self.landing {
            let still = now.saturating_duration_since(landing.last_move);
            let waited = now.saturating_duration_since(landing.since);
            if still >= LANDING_QUIET || waited >= LANDING_MAX {
                self.launch(now, config.max_length);
            }
        }
        let dt = self
            .clock
            .map_or(Duration::ZERO, |clock| now.saturating_duration_since(clock));
        self.clock = Some(now);
        let sparks = &mut self.sparks;
        if let Some(running) = self.running.as_mut() {
            let particles = config.particles;
            let mut previous = running.smear.current_center();
            running
                .smear
                .advance(dt, config.motion, config.max_length, |smear| {
                    let center = smear.current_center();
                    let travelled = (center - previous).length();
                    previous = center;
                    if let Some(particles) = particles.as_ref() {
                        sparks.emit(
                            particles,
                            |along| smear.body_point(along),
                            smear.velocity(),
                            travelled,
                        );
                    }
                });
            if running.smear.settled() {
                self.running = None;
            }
        }
        match config.particles {
            Some(particles) if self.sparks.alive() => {
                self.spark_pending += dt;
                while self.spark_pending >= motion::STEP {
                    self.spark_pending -= motion::STEP;
                    self.sparks.step(&particles, motion::STEP);
                }
            }
            Some(_) => self.spark_pending = Duration::ZERO,
            None => self.sparks.clear(),
        }
    }

    /// Advance the trail clock and say whether, and when, it needs a frame.
    pub(crate) fn tick(
        &mut self,
        config: Option<CursorTrail>,
        frame_interval: Duration,
        now: Instant,
    ) -> CursorTrailTick {
        self.advance(config, now);
        if let Some(landing) = self.landing {
            let still = now.saturating_duration_since(landing.last_move);
            let waited = now.saturating_duration_since(landing.since);
            return CursorTrailTick {
                // Nothing moves while a jump waits, but the host may still show an earlier
                // trail to clear, or not yet the held caret.
                needs_paint: self.painted != self.frame(config),
                next_due: Some(
                    LANDING_QUIET
                        .saturating_sub(still)
                        .min(LANDING_MAX.saturating_sub(waited)),
                ),
            };
        }
        if self.running.is_some() || self.sparks.alive() {
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

    /// Whether this frame must be a full draw: a smear or a spark is moving, or the host still
    /// shows one.
    pub(crate) fn requires_full_draw(&self) -> bool {
        self.running.is_some()
            || self.sparks.alive()
            || self.painted.is_some()
            || self.landing.is_some_and(|landing| landing.hold)
    }

    /// Whether the host caret must be hidden now: a solid smear, or a held caret, draws it.
    pub(crate) fn hides_caret(&self) -> bool {
        self.running.as_ref().is_some_and(|running| running.hides)
            || self.landing.is_some_and(|landing| landing.hold)
    }

    /// The trail to paint over a full draw, as of the last [`Self::advance`].
    pub(crate) fn frame(&self, config: Option<CursorTrail>) -> Option<CursorTrailFrame> {
        let config = config.filter(CursorTrail::is_visible)?;
        let smear = self
            .running
            .as_ref()
            .map(|running| shape_of(&running.smear, running.drawn))
            .or_else(|| {
                self.landing
                    .filter(|landing| landing.hold)
                    .map(|landing| held_shape(&landing.from))
            });
        let sparks = if config.particles.is_some() {
            self.sparks.snapshot()
        } else {
            Vec::new()
        };
        if smear.is_none() && sparks.is_empty() {
            return None;
        }
        // Only sparks need the caret's cell; leaving it out otherwise keeps a held frame equal to
        // itself while the caret it hides moves on.
        let caret = (!sparks.is_empty())
            .then(|| self.last.map(|caret| caret.position))
            .flatten();
        Some(CursorTrailFrame {
            smear,
            sparks,
            caret,
            fill: config.fill,
            color: self
                .running
                .as_ref()
                .and_then(|running| running.color)
                .or(self.landing.and_then(|landing| landing.color))
                .or(config.color),
            opacity: config.opacity,
            taper: config.taper,
            glyphs: config.glyphs.resolve(),
        })
    }

    /// The host's cell size in pixels, when known, so a smear's bends and taper are measured on
    /// screen rather than in cells, which are taller than they are wide.
    pub(crate) fn set_cell_size(&mut self, cell: Option<(u16, u16)>) {
        self.cell_aspect = cell
            .filter(|&(width, height)| width > 0 && height > 0)
            .map(|(width, height)| f32::from(height) / f32::from(width));
    }

    /// Record that a full draw went out, with the trail it painted.
    pub(crate) fn record_full_draw(&mut self, painted: Option<CursorTrailFrame>, now: Instant) {
        if painted.is_some() {
            self.last_frame = Some(now);
        }
        if let Some(running) = self.running.as_mut()
            && let Some(shape) = painted.as_ref().and_then(|frame| frame.smear.as_ref())
        {
            running.drawn = Some(shape.head_arc);
        }
        self.painted = painted;
    }

    /// The trail the host shows from the last full draw, for a capture of that frame.
    pub(crate) fn painted(&self) -> Option<&CursorTrailFrame> {
        self.painted.as_ref()
    }
}

/// The caret held in place, as a smear that has not moved yet.
fn held_shape(corners: &Corners) -> SmearShape {
    let center = motion::center(corners);
    SmearShape {
        pieces: vec![Hull::of(corners)],
        along: vec![1.0],
        head: center,
        tail: center,
        head_arc: 0.0,
        target: Position::new(corners[0].x.max(0.0) as u16, corners[0].y.max(0.0) as u16),
    }
}

fn shape_of(smear: &Smear, drawn: Option<f32>) -> SmearShape {
    let ribbon = smear.ribbon(drawn);
    let target = smear.target();
    SmearShape {
        pieces: ribbon.pieces.iter().map(|piece| Hull::of(piece)).collect(),
        along: ribbon.along,
        head: motion::center(target),
        tail: ribbon.tail,
        head_arc: ribbon.head_arc,
        target: Position::new(target[0].x.max(0.0) as u16, target[0].y.max(0.0) as u16),
    }
}

impl CursorTrailFrame {
    /// Whether the smear draws the caret itself this frame, so the host caret must be hidden.
    pub(crate) fn hides_caret(&self) -> bool {
        self.smear.is_some() && self.fill == CursorTrailFill::Solid
    }

    /// Paint the frame over `buffer`. `host` is what the host terminal reported about its colors:
    /// its background resolves cells left at the terminal default, its cursor and foreground
    /// colors are fallbacks for the trail's own.
    pub(crate) fn paint(&self, buffer: &mut Buffer, host: Option<HostTerminalColors>) {
        let color = self
            .color
            .or(host.and_then(|host| host.cursor))
            .or(host.map(|host| host.fg))
            .or_else(|| {
                let target = self.smear.as_ref()?.target;
                buffer.cell(target).and_then(|cell| match cell.fg {
                    ratatui::style::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
                    _ => None,
                })
            })
            .unwrap_or(Color::Gray);
        let host_bg = host.and_then(|host| match host.bg {
            Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        });
        let smear = self.smear.as_ref().map(|shape| SmearPaint {
            pieces: shape.pieces.clone(),
            along: shape.along.clone(),
            head: shape.head,
            tail: shape.tail,
            fill: self.fill,
            color,
            opacity: self.opacity,
            taper: self.taper,
            glyphs: self.glyphs,
        });
        if !self.sparks.is_empty() {
            particles::paint(
                &self.sparks,
                buffer,
                crate::backend::ratatui_backend::common::to_ratatui_color(color),
                smear.as_ref(),
                self.caret,
                host_bg,
            );
        }
        if let Some(smear) = smear {
            smear.paint(buffer, host_bg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Color as RColor;

    const RED: (u8, u8, u8) = (255, 0, 0);
    const INTERVAL: Duration = Duration::from_millis(16);

    fn node(id: u32) -> Option<CaretOwner> {
        moved(id, 0, 0)
    }

    /// Widget `id`, laid out at `(x, y)` on screen.
    fn moved(id: u32, x: i32, y: i32) -> Option<CaretOwner> {
        Some(CaretOwner {
            id: NodeId::new(id, 0),
            origin: (x, y),
        })
    }

    fn trail() -> Option<CursorTrail> {
        Some(CursorTrail::new().color(Color::Rgb(RED.0, RED.1, RED.2)))
    }

    fn block() -> CaretLook {
        CaretLook {
            shape: CaretShape::Block,
            color: None,
        }
    }

    fn see(
        state: &mut CursorTrailState,
        config: Option<CursorTrail>,
        x: u16,
        y: u16,
        owner: u32,
        at: Instant,
    ) {
        state.observe(config, Some(Position::new(x, y)), node(owner), block(), at);
    }

    fn running(state: &mut CursorTrailState, start: Instant) {
        see(state, trail(), 1, 1, 1, start);
        see(state, trail(), 11, 1, 2, start);
    }

    /// Frames from `start` until the trail ends, at 60 a second.
    fn play(
        state: &mut CursorTrailState,
        config: Option<CursorTrail>,
        start: Instant,
    ) -> Vec<CursorTrailFrame> {
        let mut frames = Vec::new();
        let mut at = start;
        for _ in 0..240 {
            let tick = state.tick(config, INTERVAL, at);
            let frame = state.frame(config);
            state.record_full_draw(frame.clone(), at);
            match frame {
                Some(frame) => frames.push(frame),
                None if !tick.needs_paint && state.landing.is_none() => break,
                None => {}
            }
            at += INTERVAL;
        }
        frames
    }

    #[test]
    fn a_focus_change_starts_a_trail_and_a_same_widget_move_does_not() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        see(&mut state, trail(), 1, 1, 1, start);
        see(&mut state, trail(), 30, 9, 1, start);
        state.advance(trail(), start + LANDING_MAX);
        assert!(state.frame(trail()).is_none(), "a move inside one widget");
        see(&mut state, trail(), 2, 2, 2, start + LANDING_MAX);
        assert!(
            state.frame(trail()).is_some(),
            "a move to another widget starts at once"
        );
    }

    #[test]
    fn jump_thresholds_gate_same_widget_moves() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see(&mut state, jump, 0, 0, 1, start);
        see(&mut state, jump, 7, 1, 1, start);
        state.advance(jump, start + LANDING_QUIET);
        assert!(state.frame(jump).is_none(), "7 columns and 1 row");
        see(&mut state, jump, 7, 3, 1, start + LANDING_QUIET);
        state.advance(jump, start + LANDING_QUIET * 2);
        assert!(state.frame(jump).is_some(), "2 rows");
    }

    /// Observe a caret at `(x, y)` in widget `owner`, laid out at `origin`.
    fn see_in(
        state: &mut CursorTrailState,
        config: Option<CursorTrail>,
        (x, y): (u16, u16),
        (owner, origin): (u32, (i32, i32)),
        at: Instant,
    ) {
        let owner = moved(owner, origin.0, origin.1);
        state.observe(config, Some(Position::new(x, y)), owner, block(), at);
    }

    #[test]
    fn a_skip_waits_for_the_move_it_skips() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        see(&mut state, trail(), 1, 1, 1, start);
        state.skip_next_move();
        // A frame that still shows the caret where it was, as a transition's first frames can,
        // does not spend the skip.
        see(&mut state, trail(), 1, 1, 1, start + INTERVAL);
        state.advance(trail(), start + INTERVAL);
        assert!(state.frame(trail()).is_none());
        see(&mut state, trail(), 11, 6, 2, start + INTERVAL * 2);
        state.advance(trail(), start + INTERVAL * 2);
        assert!(
            state.frame(trail()).is_none(),
            "the skipped move drew a trail"
        );
        see(&mut state, trail(), 1, 1, 1, start + INTERVAL * 3);
        state.advance(trail(), start + INTERVAL * 3);
        assert!(
            state.frame(trail()).is_some(),
            "the move after it trails again"
        );
    }

    #[test]
    fn a_skip_stops_a_trail_in_flight() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        state.advance(trail(), start + INTERVAL);
        assert!(state.frame(trail()).is_some());
        state.skip_next_move();
        assert!(state.frame(trail()).is_none());
    }

    #[test]
    fn a_caret_its_widget_carries_does_not_trail() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        // A pane sliding 20 columns and 4 rows a frame, its caret still in the same cell of it.
        for step in 0..5_u16 {
            let at = start + INTERVAL * u32::from(step);
            let origin = (i32::from(step) * 20, i32::from(step) * 4);
            see_in(
                &mut state,
                jump,
                (5 + step * 20, 3 + step * 4),
                (1, origin),
                at,
            );
            state.advance(jump, at + LANDING_QUIET);
            assert!(state.frame(jump).is_none(), "step {step}");
        }
    }

    #[test]
    fn a_smear_in_flight_rides_with_its_widget() {
        let start = Instant::now();
        let mut still = CursorTrailState::default();
        let mut carried = CursorTrailState::default();
        for state in [&mut still, &mut carried] {
            see_in(state, trail(), (1, 1), (1, (0, 0)), start);
            see_in(state, trail(), (11, 1), (2, (0, 0)), start);
            state.advance(trail(), start + INTERVAL);
        }
        // Mid-flight, the second widget moves 20 columns and 3 rows, taking its caret along.
        see_in(&mut still, trail(), (11, 1), (2, (0, 0)), start + INTERVAL);
        see_in(
            &mut carried,
            trail(),
            (31, 4),
            (2, (20, 3)),
            start + INTERVAL,
        );
        let at = start + INTERVAL * 2;
        still.advance(trail(), at);
        carried.advance(trail(), at);
        let shape = |state: &CursorTrailState| state.frame(trail()).and_then(|frame| frame.smear);
        let (still, carried) = (shape(&still).unwrap(), shape(&carried).unwrap());
        let offset = Vec2::new(20.0, 3.0);
        let close = |a: Vec2, b: Vec2| (a - b).length() < 1e-3;
        assert!(
            close(carried.head, still.head + offset),
            "{carried:?} vs {still:?}"
        );
        assert!(
            close(carried.tail, still.tail + offset),
            "{carried:?} vs {still:?}"
        );
        assert_eq!(carried.target, Position::new(31, 4));
    }

    #[test]
    fn a_jump_inside_a_moving_widget_trails_from_where_it_was_carried() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_in(&mut state, jump, (5, 3), (1, (0, 0)), start);
        // The widget moves 20 columns while its caret jumps 10 within it.
        see_in(&mut state, jump, (35, 3), (1, (20, 0)), start);
        let landing = state.landing.expect("a jump within the widget");
        assert_eq!(landing.from, caret_rect(25, 3, CaretShape::Block));
        assert_eq!(landing.to, caret_rect(35, 3, CaretShape::Block));
    }

    #[test]
    fn a_caret_that_lands_in_steps_gets_one_straight_trail() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        // A command typed out to column 40, then Enter: the newline lands the caret at column 0
        // of a blank line, and the prompt arrives two frames later, two rows down at column 40.
        see(&mut state, jump, 40, 3, 1, start);
        see(&mut state, jump, 0, 4, 1, start);
        let tint = jump.map(|trail| trail.fill(CursorTrailFill::Tint));
        let mut tinted = CursorTrailState::default();
        see(&mut tinted, tint, 40, 3, 1, start);
        see(&mut tinted, tint, 0, 4, 1, start);
        for at in [start, start + INTERVAL] {
            assert!(!tinted.tick(tint, INTERVAL, at).needs_paint);
            assert!(
                tinted.frame(tint).is_none(),
                "a tint draws nothing while the caret may still move"
            );
            assert!(
                !tinted.requires_full_draw(),
                "the fast paths keep running meanwhile"
            );
            // A solid trail holds the caret where it was, drawn by the smear, host caret hidden.
            state.tick(jump, INTERVAL, at);
            let held = state.frame(jump).expect("the held caret");
            assert!(held.hides_caret() && state.hides_caret());
            let points = vertices(held.smear.as_ref().expect("shape"));
            assert!(
                points
                    .iter()
                    .all(|p| p.x >= 40.0 && p.x <= 41.0 && p.y >= 3.0 && p.y <= 4.0),
                "held on the old cell: {points:?}"
            );
            state.record_full_draw(Some(held), at);
        }
        let prompt = start + Duration::from_millis(33);
        see(&mut state, jump, 40, 6, 1, prompt);
        let tick = state.tick(jump, INTERVAL, prompt);
        assert!(!tick.needs_paint, "the held caret is already on screen");
        assert_eq!(
            tick.next_due,
            Some(LANDING_QUIET),
            "waits for the caret to hold still"
        );

        let settled = prompt + LANDING_QUIET;
        assert!(state.tick(jump, INTERVAL, settled).needs_paint);
        let frame = state
            .frame(jump)
            .expect("the smear starts once the caret settles");
        let smear = frame.smear.as_ref().expect("smear");
        assert_eq!(smear.target, Position::new(40, 6));
        let points = vertices(smear);
        assert!(
            points
                .iter()
                .all(|point| point.x >= 40.0 && point.x <= 41.0),
            "straight down from column 40, never toward column 0: {points:?}"
        );

        // A caret that never holds still starts after the longest wait.
        let mut state = CursorTrailState::default();
        see(&mut state, jump, 0, 0, 1, start);
        let mut at = start;
        let mut column = 10;
        while state.running.is_none() {
            assert!(at < start + LANDING_MAX * 2, "never started");
            at += Duration::from_millis(20);
            column += 1;
            see(&mut state, jump, column, 9, 1, at);
            state.tick(jump, INTERVAL, at);
        }
        let first_jump = start + Duration::from_millis(20);
        assert!(at >= first_jump + LANDING_MAX && at < first_jump + LANDING_MAX + INTERVAL * 2);
    }

    #[test]
    fn a_hidden_caret_keeps_its_last_cell_for_the_next_trail() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        see(&mut state, trail(), 1, 1, 1, start);
        state.observe(trail(), None, node(1), block(), start);
        see(&mut state, trail(), 9, 1, 2, start);
        let frame = state
            .frame(trail())
            .expect("trail after the caret came back");
        assert!(
            vertices(frame.smear.as_ref().expect("smear"))
                .iter()
                .any(|p| p.x <= 2.0)
        );
    }

    #[test]
    fn the_trail_settles_and_asks_for_one_clearing_frame() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        let frames = play(&mut state, trail(), start);
        assert!(
            frames.len() > 3,
            "the smear takes several frames: {}",
            frames.len()
        );
        assert!(
            frames.len() < 60,
            "and settles within a second: {}",
            frames.len()
        );
        assert!(
            frames.iter().all(CursorTrailFrame::hides_caret),
            "a solid smear draws the caret"
        );
        assert!(!state.requires_full_draw(), "the clearing frame went out");
    }

    /// A fast smear reads as one streak: every frame overlaps the one before, and the first one
    /// that moves reaches back to where the caret was.
    /// A fast smear reads as one streak: every frame overlaps the one before, and the first one
    /// that moves reaches back to where the caret was. That holds at a steady 60 frames a second
    /// and when slow frames let several physics steps pass between two of them.
    #[test]
    fn a_long_jump_is_one_unbroken_streak_over_time() {
        fn bounds(frame: &CursorTrailFrame) -> (f32, f32, f32, f32) {
            vertices(frame.smear.as_ref().expect("smear")).iter().fold(
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                |(left, top, right, bottom), p| {
                    (left.min(p.x), top.min(p.y), right.max(p.x), bottom.max(p.y))
                },
            )
        }
        for motion in [CursorTrailMotion::default(), CursorTrail::glide()] {
            for interval in [INTERVAL, Duration::from_millis(50)] {
                let config = trail().map(|trail| trail.motion(motion));
                let start = Instant::now();
                let mut state = CursorTrailState::default();
                see(&mut state, config, 2, 1, 1, start);
                see(&mut state, config, 120, 30, 2, start);
                let mut frames = Vec::new();
                let mut at = start;
                for _ in 0..200 {
                    state.tick(config, interval, at);
                    let frame = state.frame(config);
                    state.record_full_draw(frame.clone(), at);
                    match frame {
                        Some(frame) if frame.smear.is_some() => frames.push(frame),
                        _ if !state.requires_full_draw() => break,
                        _ => {}
                    }
                    at += interval;
                }
                assert!(frames.len() > 2, "{motion:?} at {interval:?}");
                let moving = frames
                    .iter()
                    .find(|frame| bounds(frame).2 > 4.0)
                    .expect("a frame that moved");
                assert!(
                    bounds(moving).0 <= 3.0 && bounds(moving).1 <= 2.0,
                    "{motion:?} at {interval:?}: the first moving frame reaches back to the \
                     origin: {:?}",
                    bounds(moving)
                );
                for pair in frames.windows(2) {
                    let (a, b) = (bounds(&pair[0]), bounds(&pair[1]));
                    assert!(
                        a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3,
                        "{motion:?} at {interval:?}: consecutive frames do not overlap: {a:?} \
                         then {b:?}"
                    );
                }
            }
        }
    }

    /// Every corner of every piece the smear is drawn as.
    fn vertices(shape: &SmearShape) -> Vec<Vec2> {
        shape
            .pieces
            .iter()
            .flat_map(|piece| piece.vertices_for_tests().to_vec())
            .collect()
    }

    /// A smear sent somewhere else mid-flight, as by two focus changes in quick succession,
    /// bends along the way its head went: every piece of it is at most the caret's width across,
    /// so it never turns into a slab or a wedge, and each frame touches the one before, so it
    /// never hops. That holds at 60 frames a second and when slow frames let steps pass.
    #[test]
    fn a_smear_sent_elsewhere_mid_flight_stays_a_ribbon() {
        let block = |x, y| motion::caret_rect(x, y, crate::CaretShape::Block);
        let bar = |x, y| motion::caret_rect(x, y, crate::CaretShape::Bar);
        let moves = [
            // Down a column, then off to the left, straight sideways, or back up and over.
            (bar(100, 2), bar(100, 32), bar(30, 36)),
            (block(100, 2), block(100, 32), block(30, 32)),
            (block(100, 2), block(100, 32), block(60, 4)),
            // A long diagonal, then back down and to the left.
            (block(5, 20), block(70, 4), block(10, 30)),
        ];
        for (from, first, second) in moves {
            let caret = first[2] - first[0];
            let widest = caret.length() + 1e-3;
            for steps_per_frame in [1, 2, 4] {
                let mut smear = motion::Smear::new_for_tests(from, first);
                let mut drawn: Option<f32> = None;
                let mut last: Vec<Hull> = Vec::new();
                for frame in 0..40 {
                    if frame == 3 {
                        smear.retarget(second);
                    }
                    smear.advance(
                        motion::STEP * steps_per_frame,
                        CursorTrailMotion::default(),
                        0.0,
                        |_| {},
                    );
                    let shape = shape_of(&smear, drawn);
                    let at = format!(
                        "{:?} then {:?}, {steps_per_frame} steps per frame, frame {frame}",
                        first[0], second[0]
                    );
                    for piece in &shape.pieces {
                        let width = piece.width_for_tests();
                        assert!(width <= widest, "{at}: a piece {width} cells across");
                    }
                    if !last.is_empty() {
                        assert!(
                            shape
                                .pieces
                                .iter()
                                .any(|piece| last.iter().any(|old| piece.overlaps(old))),
                            "{at}: the streak hops"
                        );
                    }
                    drawn = Some(shape.head_arc);
                    last = shape.pieces;
                }
                assert!(
                    smear.settled(),
                    "{:?} then {:?}: settles",
                    first[0],
                    second[0]
                );
            }
        }
    }

    #[test]
    fn the_richest_glyphs_the_terminal_draws_itself_are_chosen() {
        let detect = |vars: &[(&str, &str)]| {
            detect_glyphs(|name| {
                vars.iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_string())
            })
        };
        assert_eq!(detect(&[("TERM_PROGRAM", "ghostty")]), GlyphSet::Octants);
        assert_eq!(detect(&[("TERM", "xterm-ghostty")]), GlyphSet::Octants);
        assert_eq!(detect(&[("TERM", "xterm-kitty")]), GlyphSet::Sextants);
        assert_eq!(detect(&[("TERM_PROGRAM", "WezTerm")]), GlyphSet::Sextants);
        assert_eq!(detect(&[("TERM", "foot")]), GlyphSet::Sextants);
        assert_eq!(detect(&[("TERM", "xterm-256color")]), GlyphSet::Blocks);
        assert_eq!(detect(&[]), GlyphSet::Blocks);
    }

    #[test]
    fn disabling_the_trail_stops_a_running_one() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        running(&mut state, start);
        state.tick(None, INTERVAL, start);
        assert!(state.frame(trail()).is_none());
        let zero = Some(CursorTrail::new().motion(CursorTrailMotion::Glide {
            duration: Duration::ZERO,
        }));
        running(&mut state, start);
        state.tick(zero, INTERVAL, start);
        assert!(state.frame(zero).is_none());
    }

    #[test]
    fn sparks_outlive_the_smear_and_then_clear() {
        let start = Instant::now();
        let sparky = Some(
            CursorTrail::new()
                .color(Color::Rgb(RED.0, RED.1, RED.2))
                .particles(Some(CursorTrailParticles::default())),
        );
        let mut state = CursorTrailState::default();
        see(&mut state, sparky, 1, 1, 1, start);
        see(&mut state, sparky, 30, 1, 2, start);
        let frames = play(&mut state, sparky, start);
        assert!(
            frames.iter().any(|frame| !frame.sparks.is_empty()),
            "sparks fly"
        );
        let last = frames.last().expect("frames");
        assert!(
            last.smear.is_none() && !last.sparks.is_empty(),
            "the sparks fade after the smear"
        );
        assert!(!state.requires_full_draw());
    }

    /// Sparks start on the smear itself: on a jump sent elsewhere mid-flight, every spark appears
    /// on the bent path the smear has covered by then, never ahead of it on a straight line toward
    /// where the caret lands.
    #[test]
    fn sparks_start_along_the_bent_smear() {
        let still = Some(CursorTrail::new().particles(Some(CursorTrailParticles {
            spread: 0.0,
            inherit: 0.0,
            gravity: 0.0,
            drag: 0.0,
            ..CursorTrailParticles::default()
        })));
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        // Down a column, then off to the lower left, as a focus change does across four panes.
        see(&mut state, still, 100, 2, 1, start);
        see(&mut state, still, 100, 32, 2, start);
        // What the smear has covered so far, and the sparks on screen, frame by frame.
        let mut frames: Vec<(Vec<Vec2>, Vec<Vec2>)> = Vec::new();
        let mut at = start;
        for frame in 0..240 {
            if frame == 4 {
                see(&mut state, still, 30, 36, 3, at);
            }
            let tick = state.tick(still, INTERVAL, at);
            let shown = state.frame(still);
            state.record_full_draw(shown.clone(), at);
            match shown {
                Some(shown) => {
                    let mut covered = frames.last().map(|(c, _)| c.clone()).unwrap_or_default();
                    if let Some(smear) = shown.smear.as_ref() {
                        covered.extend(vertices(smear));
                    }
                    let sparks = shown.sparks.iter().map(|spark| spark.position).collect();
                    frames.push((covered, sparks));
                }
                None if !tick.needs_paint => break,
                None => {}
            }
            at += INTERVAL;
        }
        assert!(
            frames.iter().any(|(_, sparks)| !sparks.is_empty()),
            "sparks fly"
        );
        for (index, (_, sparks)) in frames.iter().enumerate() {
            // A frame draws the smear up to one physics step behind the sparks it emitted, so
            // the next frame's smear may be the one that reaches them.
            let covered = &frames[(index + 1).min(frames.len() - 1)].0;
            for spark in sparks {
                let nearest = covered
                    .iter()
                    .map(|corner| (*corner - *spark).length())
                    .fold(f32::MAX, f32::min);
                assert!(
                    nearest <= 2.0,
                    "frame {index}: a spark at {spark:?} is {nearest} cells off the smear's path"
                );
            }
        }
    }

    #[test]
    fn a_tint_paints_backgrounds_and_keeps_the_caret() {
        let start = Instant::now();
        let tint = Some(
            CursorTrail::new()
                .fill(CursorTrailFill::Tint)
                .color(Color::Rgb(RED.0, RED.1, RED.2)),
        );
        let mut state = CursorTrailState::default();
        see(&mut state, tint, 1, 1, 1, start);
        see(&mut state, tint, 11, 1, 2, start);
        state.advance(tint, start + INTERVAL * 2);
        let frame = state.frame(tint).expect("running");
        assert!(!frame.hides_caret());
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        for cell in buffer.content.iter_mut() {
            cell.bg = RColor::Rgb(0, 0, 0);
            cell.set_symbol("x");
        }
        frame.paint(&mut buffer, None);
        assert!(
            (0..20).any(|x| matches!(buffer[(x, 1)].bg, RColor::Rgb(r, 0, 0) if r > 0)),
            "some cell is tinted"
        );
        assert!(
            (0..20).all(|x| buffer[(x, 1)].symbol() == "x"),
            "text stays"
        );
    }

    /// A trail with no color of its own takes the color of the caret it follows now: sent on from
    /// one widget to another mid-flight, or while it waits for a caret to settle, it does not keep
    /// the color of the caret it left, and where the new caret has none it falls back to the host
    /// cursor's.
    #[test]
    fn a_retargeted_trail_takes_the_color_of_the_caret_it_follows() {
        let start = Instant::now();
        let green = Color::Rgb(0, 200, 0);
        let look = |color| CaretLook {
            shape: CaretShape::Block,
            color,
        };
        let at = |x, y| Some(Position::new(x, y));
        let frame_color =
            |state: &CursorTrailState, config| state.frame(config).expect("a frame").color;

        // A focus change from 1 to a green caret in 2, then on to an uncolored caret in 3.
        let plain = Some(CursorTrail::new());
        let mut state = CursorTrailState::default();
        state.observe(plain, at(1, 1), node(1), look(None), start);
        state.observe(plain, at(30, 1), node(2), look(Some(green)), start);
        state.advance(plain, start + INTERVAL);
        assert_eq!(frame_color(&state, plain), Some(green));
        state.observe(plain, at(30, 20), node(3), look(None), start + INTERVAL);
        state.advance(plain, start + INTERVAL * 2);
        assert_eq!(
            frame_color(&state, plain),
            None,
            "the uncolored caret leaves the trail to the host cursor color"
        );

        // A jump inside one widget that waits to settle, the caret taking a color meanwhile.
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        state.observe(jump, at(0, 0), node(1), look(None), start);
        state.observe(jump, at(40, 3), node(1), look(None), start);
        state.observe(
            jump,
            at(40, 6),
            node(1),
            look(Some(green)),
            start + INTERVAL,
        );
        assert_eq!(frame_color(&state, jump), Some(green), "while it waits");
        state.advance(jump, start + INTERVAL + LANDING_QUIET);
        assert!(state.running.is_some(), "the smear started");
        assert_eq!(frame_color(&state, jump), Some(green), "once it runs");
    }

    #[test]
    fn the_trail_takes_the_host_cursor_color_when_it_has_none_of_its_own() {
        let start = Instant::now();
        let plain = Some(CursorTrail::new());
        let mut state = CursorTrailState::default();
        see(&mut state, plain, 1, 1, 1, start);
        see(&mut state, plain, 11, 1, 2, start);
        state.advance(plain, start);
        let frame = state.frame(plain).expect("running");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        let host = HostTerminalColors {
            ansi: [Color::Black; 16],
            ansi_reported: 0,
            fg: Color::Rgb(1, 2, 3),
            bg: Color::Rgb(0, 0, 0),
            cursor: Some(Color::Rgb(9, 99, 199)),
        };
        frame.paint(&mut buffer, Some(host));
        assert!(
            buffer.content.iter().any(
                |cell| cell.bg == RColor::Rgb(9, 99, 199) || cell.fg == RColor::Rgb(9, 99, 199)
            ),
            "painted in the host's cursor color"
        );
    }
}
