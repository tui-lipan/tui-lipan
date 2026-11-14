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
use crate::core::event::MouseKind;
use crate::core::node::NodeId;
use crate::style::{Color, HostTerminalColors};

use motion::{Corners, Smear, caret_rect};
use particles::{Spark, Sparks};
use raster::{GlyphSet, Hull, SmearPaint};

/// What makes a [`CursorTrail`] appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorTrailTrigger {
    /// Only when focus moves to another widget and the caret moves with it. A keyed widget stays
    /// the same widget when a view change rebuilds its node, such as wrapping it in a container.
    #[default]
    FocusChange,
    /// On a focus change, and whenever the caret moves at least `columns` columns or `rows` rows
    /// in one step inside the same widget. The move is measured within the widget, so a widget
    /// that moves (an animated layout, a drag) carries its caret without starting a trail. A
    /// widget that changes size (or a terminal whose screen does) reflows its content, so for
    /// 300 ms after it does, moves inside it start none, and a jump trail inside it stops. Moves
    /// the pointer makes inside a widget (a click, a drag, a scroll) start none either.
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

/// How long a jump inside one widget waits for the caret to settle before its smear starts.
///
/// By default ([`Self::DEFAULT`]) nothing waits: every jump starts its smear at once rather than
/// holding the caret on its old cell while it settles, and a caret that moves on retargets the
/// running smear at once. A shell's
/// newline and then its prompt, written apart, draw one smear that follows the caret on to the
/// prompt: straight there when the prompt comes before the smear has set off for the new line,
/// bending toward it when the smear is already on its way.
///
/// With a `quiet` above zero ([`Self::SHELL`]), a jump to another row or to the widget's left edge,
/// which may be the first of several writes, waits until the caret has held still for `quiet`, or
/// `max` after the jump at the latest, and its smear runs straight to where the caret came to rest.
/// A solid trail holds the caret where it was meanwhile. A caret that goes on along its new row
/// instead (a held arrow key wrapping onto another line) has landed, and its smear starts then. A
/// jump along one row and a focus change always start at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorTrailSettle {
    /// How long the caret must hold still. Zero (the default) starts every jump at once.
    pub quiet: Duration,
    /// The longest a jump waits for a caret that keeps moving.
    pub max: Duration,
}

impl CursorTrailSettle {
    /// No wait: every jump starts at once.
    pub const DEFAULT: Self = Self {
        quiet: Duration::ZERO,
        max: Duration::from_millis(200),
    };

    /// Waits longer than the gap between a shell's newline and its prompt, so the two draw one
    /// straight smear.
    pub const SHELL: Self = Self {
        quiet: Duration::from_millis(50),
        max: Duration::from_millis(200),
    };

    /// Whether a jump that has waited `waited`, and held still for `still`, starts now.
    fn settled(self, still: Duration, waited: Duration) -> bool {
        still >= self.quiet || waited >= self.max
    }

    /// How long until a jump that has waited `waited`, and held still for `still`, starts.
    fn due(self, still: Duration, waited: Duration) -> Duration {
        self.quiet
            .saturating_sub(still)
            .min(self.max.saturating_sub(waited))
    }
}

impl Default for CursorTrailSettle {
    fn default() -> Self {
        Self::DEFAULT
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
    /// How long a jump that may land in steps waits for the caret to settle.
    pub settle: CursorTrailSettle,
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
            settle: CursorTrailSettle::DEFAULT,
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

    /// Set how long a jump that may land in steps waits for the caret to settle.
    #[must_use]
    pub const fn settle(mut self, settle: CursorTrailSettle) -> Self {
        self.settle = settle;
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

/// Which widget owns the caret, stable across a rebuild of its node.
///
/// A view that wraps a subtree in a new container (a sidebar docking beside the content, say)
/// rebuilds every node in it, so a node id alone would read the same widget as another one. A
/// keyed widget is the same widget for as long as its key is, which is what focus restore goes by
/// too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CaretOwnerId {
    /// A hash of the widget's key.
    Key(u64),
    /// An unkeyed widget's node.
    Node(NodeId),
}

impl CaretOwnerId {
    pub(crate) fn of(id: NodeId, key: Option<&crate::core::element::Key>) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault};
        match key {
            Some(key) => Self::Key(
                BuildHasherDefault::<rustc_hash::FxHasher>::default().hash_one(key.as_ref()),
            ),
            None => Self::Node(id),
        }
    }
}

/// The widget that owns the caret, and where its layout rectangle sits on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretOwner {
    pub(crate) id: CaretOwnerId,
    pub(crate) origin: (i32, i32),
    /// The widget's width and height in cells.
    pub(crate) size: (u16, u16),
    /// The columns and rows of the grid the caret moves in, when the widget keeps one apart from
    /// its layout: a terminal's screen, which may resize a debounce after its widget does.
    pub(crate) grid: Option<(u16, u16)>,
}

impl CaretOwner {
    /// Whether the cell at `(x, y)` is inside the widget as it paints.
    fn contains(self, (x, y): (u16, u16)) -> bool {
        let (x, y) = (i32::from(x), i32::from(y));
        let (left, top) = self.origin;
        x >= left
            && y >= top
            && x < left + i32::from(self.size.0)
            && y < top + i32::from(self.size.1)
    }
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

    fn owner_id(self) -> Option<CaretOwnerId> {
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

    /// Whether the widget that owns the caret is the one that owned `earlier`, and it or the
    /// grid the caret moves in changed size since.
    fn resized_from(self, earlier: Self) -> bool {
        matches!(
            (earlier.owner, self.owner),
            (Some(from), Some(to))
                if from.id == to.id && (from.size != to.size || from.grid != to.grid)
        )
    }

    /// Whether the caret moved along its row since `earlier`, inside the same widget, rather than
    /// being carried there by it: a held arrow key, or a prompt being written out.
    fn moved_along_row(self, earlier: Self) -> bool {
        if earlier.owner_id() != self.owner_id() {
            return false;
        }
        let (carried_x, carried_y) = self.carried_from(earlier);
        let dx = i32::from(self.position.x) - i32::from(earlier.position.x) - carried_x;
        let dy = i32::from(self.position.y) - i32::from(earlier.position.y) - carried_y;
        dy == 0 && dx != 0
    }

    /// Whether a jump inside one widget from `earlier` may be the first of several writes: it
    /// left the row, as a newline does, or reached the widget's left edge, as a carriage return
    /// does.
    fn may_land_in_steps(self, earlier: Self) -> bool {
        let (_, carried_y) = self.carried_from(earlier);
        let row_changed = i32::from(self.position.y) - i32::from(earlier.position.y) != carried_y;
        let at_left_edge = self
            .owner
            .is_some_and(|owner| i32::from(self.position.x) <= owner.origin.0);
        row_changed || at_left_edge
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
    /// The caret jumped inside its widget, rather than leaving it for another.
    within: bool,
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
    /// The smear follows a jump inside one widget, rather than a focus change.
    within: bool,
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
    /// The widget the caret is in and when it last changed size, while its content may still
    /// be reflowing.
    resized: Option<(CaretOwnerId, Instant)>,
    /// The widget the pointer last pressed, dragged, released, or scrolled in while it held the
    /// caret, and when.
    pointed: Option<(CaretOwnerId, Instant)>,
    /// The widget a pointer press that is still held began in, while it held the caret, so a drag
    /// past its edge still acts on it.
    pressed: Option<CaretOwnerId>,
}

impl CursorTrailState {
    /// How long after the caret's widget changes size a move inside it starts no trail. A
    /// resized terminal reflows when its program redraws, a few frames after the resize, so the
    /// quiet outlasts that round trip.
    pub(crate) const REFLOW_QUIET: Duration = Duration::from_millis(300);

    /// How long after pointer input a move inside one widget starts no trail. A caret the pointer
    /// places, drags, or scrolls lands where the user is already looking, a frame or two after
    /// the input that moved it.
    pub(crate) const POINTER_QUIET: Duration = Duration::from_millis(150);

    /// Record pointer input at `at` that may move the caret in `owner`, the widget that holds it:
    /// a press or scroll inside it, or a drag or release of a press that began inside it. Input
    /// anywhere else leaves the caret's moves to trail as usual.
    pub(crate) fn note_pointer(
        &mut self,
        now: Instant,
        owner: Option<CaretOwner>,
        at: (u16, u16),
        kind: MouseKind,
    ) {
        let inside = owner
            .filter(|owner| owner.contains(at))
            .map(|owner| owner.id);
        let pressed = owner
            .map(|owner| owner.id)
            .filter(|&id| self.pressed == Some(id));
        let target = match kind {
            MouseKind::Moved => return,
            MouseKind::Down(_) => {
                self.pressed = inside;
                inside
            }
            MouseKind::Drag(_) => inside.or(pressed),
            MouseKind::Up(_) => {
                self.pressed = None;
                inside.or(pressed)
            }
            _ => inside,
        };
        if let Some(id) = target {
            self.pointed = Some((id, now));
        }
    }

    /// Whether the caret's move from `last` to `caret` starts a trail: it triggers one, and is
    /// neither a resized widget's content reflowing nor the pointer moving it within its widget.
    fn starts_trail(&self, config: CursorTrail, last: Caret, caret: Caret, now: Instant) -> bool {
        config.triggers(last, caret)
            && !self.reflowing(last, caret, now)
            && !self.pointer_moved(last, caret, now)
    }

    /// Whether the move from `last` to `caret` is inside one widget, moments after pointer input
    /// in it.
    fn pointer_moved(&self, last: Caret, caret: Caret, now: Instant) -> bool {
        last.owner_id() == caret.owner_id()
            && self.pointed.is_some_and(|(id, at)| {
                caret.owner_id() == Some(id)
                    && now.saturating_duration_since(at) < Self::POINTER_QUIET
            })
    }

    /// Whether the move from `last` to `caret` is inside a widget that changed size moments ago,
    /// and so may be its content reflowing rather than the caret jumping.
    fn reflowing(&self, last: Caret, caret: Caret, now: Instant) -> bool {
        last.owner_id() == caret.owner_id()
            && self.resized.is_some_and(|(id, at)| {
                caret.owner_id() == Some(id)
                    && now.saturating_duration_since(at) < Self::REFLOW_QUIET
            })
    }

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
        if caret.resized_from(last) {
            self.resized = caret.owner_id().map(|id| (id, now));
            // A jump inside the widget, waiting to land or on its way, was measured in the old
            // layout, and so were the sparks it threw: the caret's next moves are the content
            // reflowing into the new one. A trail that brought focus here keeps following the
            // caret to where it settles.
            if self.running.as_ref().is_some_and(|running| !running.within) {
                self.landing = None;
            } else {
                self.stop();
            }
        }
        if self.skip {
            // Nothing starts while a skip is pending, so there is nothing to move on either.
            if self.starts_trail(config, last, caret, now) {
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
            // Sent on to another widget, it now follows a focus change.
            if last.owner_id() != caret.owner_id() {
                running.within = false;
            }
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
                landing.within = false;
                self.launch(now, config.max_length);
            } else if caret.moved_along_row(last) {
                // The caret went on along its new row: something is driving it there (a held
                // arrow key wrapping onto another line, a prompt being written out), so it has
                // landed. Waiting for it to hold still would leave it held on the old cell for
                // as long as the key is held.
                self.launch(now, config.max_length);
            }
            return;
        }
        if self.starts_trail(config, last, caret, now) {
            self.landing = Some(Landing {
                from: carry_corners(last.rect(), carried),
                to: target,
                since: now,
                last_move: now,
                color,
                hold: config.fill == CursorTrailFill::Solid,
                within: last.owner_id() == caret.owner_id(),
            });
            // A focus change lands in one step and starts at once, and so does a jump along one
            // row (deleting words, a jump to the line's end). A jump to another row or to the
            // widget's left edge may be a newline or a carriage return with the prompt still to
            // come, so it waits for the caret to settle.
            let waits = last.owner_id() == caret.owner_id()
                && caret.may_land_in_steps(last)
                && !config.settle.quiet.is_zero();
            if !waits {
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
                within: landing.within,
            });
            self.clock = Some(now);
            self.last_frame = None;
        }
    }

    /// Move everything the trail is drawing, or waiting to draw, inside the caret's widget by
    /// `offset` cells. A trail that brought focus here starts in the widget it left, which did not
    /// move: its head follows the caret on its own, and its tail and sparks stay where they are.
    fn carry(&mut self, offset: Vec2) {
        if let Some(landing) = self.landing.as_mut().filter(|landing| landing.within) {
            landing.from = carry_corners(landing.from, offset);
            landing.to = carry_corners(landing.to, offset);
        }
        match self.running.as_mut() {
            Some(running) if !running.within => return,
            Some(running) => running.smear.carry(offset),
            None => {}
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
            if config.settle.settled(still, waited) {
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
                next_due: config.map(|config| config.settle.due(still, waited)),
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
    use crate::core::event::MouseButton;
    use ratatui::layout::Rect;
    use ratatui::style::Color as RColor;

    const RED: (u8, u8, u8) = (255, 0, 0);
    const LANDING_QUIET: Duration = CursorTrailSettle::SHELL.quiet;
    const LANDING_MAX: Duration = CursorTrailSettle::SHELL.max;
    const INTERVAL: Duration = Duration::from_millis(16);

    fn node(id: u32) -> Option<CaretOwner> {
        moved(id, 0, 0)
    }

    /// Widget `id`, laid out at `(x, y)` on screen.
    fn moved(id: u32, x: i32, y: i32) -> Option<CaretOwner> {
        Some(CaretOwner {
            id: CaretOwnerId::Node(NodeId::new(id, 0)),
            origin: (x, y),
            size: (80, 24),
            grid: None,
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

    /// Observe a caret at `(x, y)` in widget `owner`, sized `size`.
    fn see_sized(
        state: &mut CursorTrailState,
        config: Option<CursorTrail>,
        (x, y): (u16, u16),
        (owner, size): (u32, (u16, u16)),
        at: Instant,
    ) {
        let owner = node(owner).map(|owner| CaretOwner { size, ..owner });
        state.observe(config, Some(Position::new(x, y)), owner, block(), at);
    }

    #[test]
    fn a_resized_widget_reflowing_its_caret_starts_no_trail() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (2, 20), (1, (80, 24)), start);
        // The widget shrinks, and its program redraws the prompt ten rows up a few frames later.
        see_sized(&mut state, jump, (2, 20), (1, (80, 14)), start + INTERVAL);
        let redraw = start + INTERVAL * 4;
        see_sized(&mut state, jump, (2, 10), (1, (80, 14)), redraw);
        state.advance(jump, redraw + LANDING_MAX);
        assert!(state.frame(jump).is_none(), "the reflow is not a jump");

        let later = start + INTERVAL + CursorTrailState::REFLOW_QUIET;
        see_sized(&mut state, jump, (40, 2), (1, (80, 14)), later);
        state.advance(jump, later);
        assert!(
            state.frame(jump).is_some(),
            "a jump once the widget has held its size trails again"
        );
    }

    #[test]
    fn a_focus_change_right_after_a_resize_still_trails() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (2, 20), (1, (80, 24)), start);
        see_sized(&mut state, jump, (2, 10), (1, (80, 14)), start + INTERVAL);
        see_sized(
            &mut state,
            jump,
            (50, 3),
            (2, (40, 14)),
            start + INTERVAL * 2,
        );
        state.advance(jump, start + INTERVAL * 2);
        assert!(state.frame(jump).is_some());
    }

    #[test]
    fn a_resize_drops_a_jump_still_waiting_to_land() {
        let start = Instant::now();
        let jump = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .settle(CursorTrailSettle::SHELL),
        );
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (30, 2), (1, (80, 24)), start);
        // A newline sends the caret to the next rows' left edge, which waits to settle...
        see_sized(&mut state, jump, (0, 5), (1, (80, 24)), start + INTERVAL);
        assert!(state.landing.is_some());
        // ...and the widget is resized before it does.
        see_sized(
            &mut state,
            jump,
            (0, 5),
            (1, (60, 24)),
            start + INTERVAL * 2,
        );
        state.advance(jump, start + INTERVAL * 2 + LANDING_MAX);
        assert!(state.frame(jump).is_none());
    }

    #[test]
    fn a_resize_stops_a_jump_trail_on_its_way() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (2, 20), (1, (80, 24)), start);
        // A jump along one row starts at once.
        see_sized(&mut state, jump, (40, 20), (1, (80, 24)), start + INTERVAL);
        assert!(state.running.is_some());
        // The widget is resized mid-flight, and the caret reflows into the new layout.
        see_sized(
            &mut state,
            jump,
            (40, 20),
            (1, (80, 14)),
            start + INTERVAL * 2,
        );
        see_sized(
            &mut state,
            jump,
            (40, 10),
            (1, (80, 14)),
            start + INTERVAL * 4,
        );
        state.advance(jump, start + INTERVAL * 5);
        assert!(
            state.frame(jump).is_none(),
            "no smear is sent on to the reflowed caret"
        );
    }

    #[test]
    fn a_focus_trail_follows_its_caret_through_a_resize() {
        let start = Instant::now();
        let mut state = CursorTrailState::default();
        see_sized(&mut state, trail(), (2, 2), (1, (40, 24)), start);
        // Focus moves to a widget that is still growing into its place, as an opening pane does.
        see_sized(
            &mut state,
            trail(),
            (50, 20),
            (2, (20, 12)),
            start + INTERVAL,
        );
        see_sized(
            &mut state,
            trail(),
            (50, 22),
            (2, (30, 18)),
            start + INTERVAL * 2,
        );
        state.advance(trail(), start + INTERVAL * 2);
        assert!(
            state.frame(trail()).is_some(),
            "the trail that brought focus here keeps going"
        );
    }

    #[test]
    fn a_jump_trail_sent_on_to_another_widget_follows_it_through_a_resize() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (2, 2), (1, (80, 24)), start);
        see_sized(&mut state, jump, (40, 2), (1, (80, 24)), start + INTERVAL);
        // Focus moves on before the jump's smear settles, to a widget still growing into place.
        see_sized(
            &mut state,
            jump,
            (50, 20),
            (2, (20, 12)),
            start + INTERVAL * 2,
        );
        see_sized(
            &mut state,
            jump,
            (50, 22),
            (2, (30, 18)),
            start + INTERVAL * 3,
        );
        state.advance(jump, start + INTERVAL * 3);
        assert!(state.frame(jump).is_some());
    }

    #[test]
    fn a_resize_clears_the_sparks_of_the_jump_it_stops() {
        let start = Instant::now();
        let sparky = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .particles(Some(CursorTrailParticles {
                    per_cell: 4.0,
                    ..CursorTrailParticles::default()
                })),
        );
        let mut state = CursorTrailState::default();
        see_sized(&mut state, sparky, (2, 20), (1, (80, 24)), start);
        see_sized(
            &mut state,
            sparky,
            (60, 20),
            (1, (80, 24)),
            start + INTERVAL,
        );
        let mut at = start + INTERVAL;
        for _ in 0..3 {
            at += INTERVAL;
            state.tick(sparky, INTERVAL, at);
            let frame = state.frame(sparky);
            state.record_full_draw(frame, at);
        }
        assert!(state.sparks.alive(), "the jump threw sparks");
        see_sized(&mut state, sparky, (60, 20), (1, (80, 14)), at + INTERVAL);
        state.advance(sparky, at + INTERVAL * 2);
        assert!(state.frame(sparky).is_none());
    }

    #[test]
    fn a_grid_resized_after_its_widget_restarts_the_quiet() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let grid = |cols, rows| {
            node(1).map(|owner| CaretOwner {
                size: (60, 24),
                grid: Some((cols, rows)),
                ..owner
            })
        };
        let mut state = CursorTrailState::default();
        let at = |x, y| Some(Position::new(x, y));
        state.observe(jump, at(2, 20), grid(80, 24), block(), start);
        // The widget narrows at once; its screen follows a long debounce later, after the
        // widget's own quiet has passed, and reflows the caret as it does.
        let debounced = start + CursorTrailState::REFLOW_QUIET * 2;
        state.observe(jump, at(2, 20), grid(80, 24), block(), debounced);
        state.observe(jump, at(2, 10), grid(60, 24), block(), debounced + INTERVAL);
        state.advance(jump, debounced + INTERVAL + LANDING_MAX);
        assert!(state.frame(jump).is_none());
    }

    #[test]
    fn a_caret_the_pointer_moves_inside_its_widget_starts_no_trail() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see(&mut state, jump, 2, 2, 1, start);
        // A press in the widget, then a drag that moves the caret far, a frame after each step of
        // the pointer, and on past the widget's edge.
        let mut at = start;
        state.note_pointer(at, node(1), (2, 2), MouseKind::Down(MouseButton::Left));
        for x in [20, 40, 60] {
            state.note_pointer(at, node(1), (x + 30, 2), MouseKind::Drag(MouseButton::Left));
            at += INTERVAL;
            see(&mut state, jump, x, 2, 1, at);
            state.advance(jump, at);
            assert!(state.frame(jump).is_none(), "a drag to column {x}");
        }
        state.note_pointer(at, node(1), (90, 2), MouseKind::Up(MouseButton::Left));
        // A click on another widget is a focus change, and trails as one.
        state.note_pointer(at, node(1), (5, 30), MouseKind::Down(MouseButton::Left));
        see(&mut state, jump, 5, 9, 2, at + INTERVAL);
        state.advance(jump, at + INTERVAL);
        assert!(state.frame(jump).is_some(), "a click to another widget");
        // Once the pointer rests, a jump by keyboard trails again.
        let later = at + CursorTrailState::POINTER_QUIET * 2;
        state.advance(jump, later);
        see(&mut state, jump, 40, 9, 2, later);
        state.advance(jump, later + LANDING_QUIET);
        assert!(
            state.frame(jump).is_some(),
            "a keyboard jump after the drag"
        );
    }

    #[test]
    fn pointer_input_elsewhere_leaves_the_caret_s_jumps_trailing() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        // The widget fills columns 0..80 and rows 0..24; the clicks and scroll land below it.
        see(&mut state, jump, 2, 2, 1, start);
        state.note_pointer(start, node(1), (5, 30), MouseKind::Down(MouseButton::Left));
        state.note_pointer(start, node(1), (5, 30), MouseKind::Up(MouseButton::Left));
        state.note_pointer(start, node(1), (5, 30), MouseKind::ScrollDown);
        see(&mut state, jump, 40, 2, 1, start + INTERVAL);
        state.advance(jump, start + INTERVAL);
        assert!(state.frame(jump).is_some());
    }

    #[test]
    fn a_reflow_does_not_spend_a_skip() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut state = CursorTrailState::default();
        see_sized(&mut state, jump, (2, 20), (1, (80, 24)), start);
        state.skip_next_move();
        // The widget resizes and its caret reflows before the skipped move comes.
        see_sized(&mut state, jump, (2, 20), (1, (80, 14)), start + INTERVAL);
        see_sized(
            &mut state,
            jump,
            (2, 10),
            (1, (80, 14)),
            start + INTERVAL * 2,
        );
        assert!(state.skip, "the reflow is not the move the skip waits for");
        see_sized(
            &mut state,
            jump,
            (50, 3),
            (2, (40, 14)),
            start + INTERVAL * 3,
        );
        state.advance(jump, start + INTERVAL * 3);
        assert!(state.frame(jump).is_none(), "the focus change is skipped");
        assert!(!state.skip, "and spends the skip");
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
    fn a_smear_in_flight_inside_a_widget_rides_with_it() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        let mut still = CursorTrailState::default();
        let mut carried = CursorTrailState::default();
        for state in [&mut still, &mut carried] {
            see_in(state, jump, (1, 1), (1, (0, 0)), start);
            see_in(state, jump, (21, 1), (1, (0, 0)), start);
            state.advance(jump, start + INTERVAL);
        }
        // Mid-flight, the widget moves 20 columns and 3 rows, taking its caret along.
        see_in(&mut still, jump, (21, 1), (1, (0, 0)), start + INTERVAL);
        see_in(&mut carried, jump, (41, 4), (1, (20, 3)), start + INTERVAL);
        let at = start + INTERVAL * 2;
        still.advance(jump, at);
        carried.advance(jump, at);
        let shape = |state: &CursorTrailState| state.frame(jump).and_then(|frame| frame.smear);
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
        assert_eq!(carried.target, Position::new(41, 4));
    }

    #[test]
    fn a_focus_trail_into_a_sliding_widget_keeps_its_tail_where_focus_left() {
        let start = Instant::now();
        let mut sliding = CursorTrailState::default();
        // Focus moves from a caret at row 5 to a widget sliding in from below, as a scratchpad
        // does: it is 10 rows down at first and rises 2 rows a frame, its caret ending at row 2.
        see_in(&mut sliding, trail(), (5, 5), (1, (0, 0)), start);
        see_in(&mut sliding, trail(), (40, 12), (2, (30, 10)), start);
        for step in 1..=5_u16 {
            let at = start + INTERVAL * u32::from(step);
            let rise = i32::from(step) * 2;
            see_in(
                &mut sliding,
                trail(),
                (40, 12 - step * 2),
                (2, (30, 10 - rise)),
                at,
            );
            sliding.advance(trail(), at);
            let smear = sliding
                .frame(trail())
                .and_then(|frame| frame.smear)
                .expect("a trail in flight");
            assert!(
                smear.tail.y >= 5.0,
                "step {step}: the tail rose with the widget: {smear:?}"
            );
        }
        let landed = sliding
            .frame(trail())
            .and_then(|frame| frame.smear)
            .unwrap();
        assert_eq!(landed.target, Position::new(40, 2));
    }

    #[test]
    fn a_jump_inside_a_moving_widget_trails_from_where_it_was_carried() {
        let start = Instant::now();
        let jump = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .settle(CursorTrailSettle::SHELL),
        );
        let mut state = CursorTrailState::default();
        see_in(&mut state, jump, (5, 3), (1, (0, 0)), start);
        // The widget moves 20 columns while its caret jumps 10 within it, onto the next row.
        see_in(&mut state, jump, (35, 4), (1, (20, 0)), start);
        let landing = state.landing.expect("a jump within the widget");
        assert_eq!(landing.from, caret_rect(25, 3, CaretShape::Block));
        assert_eq!(landing.to, caret_rect(35, 4, CaretShape::Block));
    }

    #[test]
    fn a_caret_that_lands_in_steps_gets_one_straight_trail() {
        let start = Instant::now();
        let jump = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .settle(CursorTrailSettle::SHELL),
        );
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

        // A caret that never holds still, output streaming in a line at a time, starts after the
        // longest wait.
        let mut state = CursorTrailState::default();
        see(&mut state, jump, 10, 0, 1, start);
        let mut at = start;
        let mut row = 8;
        while state.running.is_none() {
            assert!(at < start + LANDING_MAX * 2, "never started");
            at += Duration::from_millis(20);
            row += 1;
            see(&mut state, jump, 0, row, 1, at);
            state.tick(jump, INTERVAL, at);
        }
        let first_jump = start + Duration::from_millis(20);
        assert!(at >= first_jump + LANDING_MAX && at < first_jump + LANDING_MAX + INTERVAL * 2);
    }

    #[test]
    fn a_jump_along_one_row_starts_at_once_and_keeps_up_with_key_repeat() {
        let start = Instant::now();
        let jump = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .settle(CursorTrailSettle::SHELL),
        );
        let mut state = CursorTrailState::default();
        // Ctrl+Backspace held in a field laid out at column 5: a word gone every 30ms, never
        // holding still for as long as a newline's prompt may take.
        let owner = 7;
        let observe = |state: &mut CursorTrailState, x: u16, at: Instant| {
            state.observe(
                jump,
                Some(Position::new(x, 3)),
                moved(owner, 5, 2),
                block(),
                at,
            );
        };
        observe(&mut state, 70, start);
        let mut at = start;
        for column in [60, 50, 40, 30] {
            at += Duration::from_millis(30);
            observe(&mut state, column, at);
            state.tick(jump, INTERVAL, at);
            let frame = state
                .frame(jump)
                .expect("the smear runs from the first word");
            let smear = frame.smear.as_ref().expect("smear");
            assert_eq!(smear.target, Position::new(column, 3), "follows each word");
        }

        // A carriage return lands on the widget's left edge and may have its prompt to come.
        let mut state = CursorTrailState::default();
        observe(&mut state, 40, start);
        observe(&mut state, 5, start);
        assert!(state.running.is_none() && state.landing.is_some());
    }

    #[test]
    fn a_held_arrow_key_wrapping_onto_another_line_is_not_held_back() {
        let start = Instant::now();
        let jump = Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::jump())
                .settle(CursorTrailSettle::SHELL),
        );
        const REPEAT: Duration = Duration::from_millis(33);
        // A command wrapped over two rows of an 80-column widget, the arrow held at key repeat.
        for (wrap, along, next) in [
            // Left from the start of the second row lands on the end of the first.
            ((0, 5), (79, 4), (78, 4)),
            // Right from the end of the first row lands on the start of the second.
            ((79, 4), (0, 5), (1, 5)),
        ] {
            let mut state = CursorTrailState::default();
            see(&mut state, jump, wrap.0, wrap.1, 1, start);
            see(&mut state, jump, along.0, along.1, 1, start + REPEAT);
            assert!(
                state.landing.is_some(),
                "the wrap may be a newline's first step"
            );
            let repeat = start + REPEAT * 2;
            see(&mut state, jump, next.0, next.1, 1, repeat);
            state.tick(jump, INTERVAL, repeat);
            let frame = state.frame(jump).expect("the next repeat starts the smear");
            let smear = frame.smear.as_ref().expect("smear");
            assert_eq!(
                smear.target,
                Position::new(next.0, next.1),
                "{wrap:?}: the smear follows the caret, a repeat after the wrap"
            );
        }
    }

    /// With no settle, a newline starts its smear at once, so the caret is never held on its old
    /// cell, and the smear follows the caret on to the prompt. Where it goes depends on whether it
    /// set off for the new line first: a prompt in before the smear's first step leaves it running
    /// straight from the command to the prompt, and one that comes after it has moved bends it.
    #[test]
    fn by_default_a_newline_starts_at_once_and_follows_on_to_its_prompt() {
        let start = Instant::now();
        let jump = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
        // Every point the smear covers, from Enter at column 40 (the newline landing at column 0
        // of the next row) until it settles on the prompt, which comes `frames` frames later.
        let covered = |frames: u32| {
            let mut state = CursorTrailState::default();
            see(&mut state, jump, 40, 3, 1, start);
            see(&mut state, jump, 0, 4, 1, start);
            assert!(
                state.landing.is_none() && state.running.is_some(),
                "nothing waits, so the caret is never held on its old cell"
            );
            let mut points = Vec::new();
            let mut at = start;
            for frame in 0..240 {
                if frame == frames {
                    see(&mut state, jump, 40, 6, 1, at);
                }
                state.tick(jump, INTERVAL, at);
                let shown = state.frame(jump);
                state.record_full_draw(shown.clone(), at);
                match shown.and_then(|shown| shown.smear) {
                    Some(smear) => {
                        if frame >= frames {
                            assert_eq!(smear.target, Position::new(40, 6), "follows the caret");
                        }
                        points.extend(vertices(&smear));
                    }
                    None if frame > frames => break,
                    None => {}
                }
                at += INTERVAL;
            }
            points
        };

        let straight = covered(0);
        assert!(
            straight.iter().all(|p| p.x >= 40.0 && p.x <= 41.0),
            "a prompt in before the first step: straight down from column 40, never toward \
             column 0: {straight:?}"
        );
        let bent = covered(3);
        assert!(
            bent.iter().any(|p| p.x < 36.0),
            "a prompt three frames on: the smear had set off toward column 0: {bent:?}"
        );
    }

    #[test]
    fn the_settle_sets_how_long_a_newline_waits() {
        let start = Instant::now();
        let jump = CursorTrail::new().trigger(CursorTrailTrigger::jump());
        let newline = |settle: CursorTrailSettle| {
            let config = Some(jump.settle(settle));
            let mut state = CursorTrailState::default();
            see(&mut state, config, 40, 3, 1, start);
            see(&mut state, config, 0, 4, 1, start);
            (state, config)
        };

        let (state, _) = newline(CursorTrailSettle {
            quiet: Duration::ZERO,
            max: LANDING_MAX,
        });
        assert!(state.running.is_some(), "no quiet starts at once");

        let quiet = Duration::from_millis(120);
        let (mut state, config) = newline(CursorTrailSettle {
            quiet,
            max: Duration::from_millis(500),
        });
        let tick = state.tick(config, INTERVAL, start);
        assert_eq!(tick.next_due, Some(quiet));
        state.tick(config, INTERVAL, start + LANDING_QUIET);
        assert!(state.running.is_none(), "still waiting past the default");
        state.tick(config, INTERVAL, start + quiet);
        assert!(state.running.is_some(), "starts once the caret held still");
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
