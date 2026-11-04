//! How the smear moves: four corners chasing the caret's rectangle.
//!
//! The smear is the caret's own rectangle with each corner free to lag behind. When the caret
//! jumps, every corner is pulled toward the matching corner of the new rectangle. The corners
//! facing the direction of travel are pulled hardest, so the shape stretches out behind the caret
//! and snaps back into it. That is the model smear-cursor.nvim made popular, and its parameters
//! mean the same here: a spring constant per corner and a velocity damping, both applied once per
//! sixtieth of a second.
//!
//! Integration runs on that fixed step whatever the frame rate, and a frame between two steps is
//! interpolated, so the same jump moves identically at 30 and 144 frames a second.

use std::time::Duration;

use super::{CursorTrailMotion, CursorTrailSpring, Vec2, ease_out_cubic};

/// The physics step. Spring constants are per step, as in smear-cursor.nvim's 17 ms loop.
pub(crate) const STEP: Duration = Duration::from_nanos(16_666_667);
/// A stall longer than this (a suspended process, a debugger) settles the smear at once rather
/// than replaying every step it missed.
const MAX_STEPS_PER_ADVANCE: u32 = 120;
/// The furthest a corner may fly past its target along the direction of travel, in cells. A
/// spring's overshoot grows with the jump, so a long one would otherwise land the smear on text
/// beyond the caret; capped, a long jump still snaps, and a short one keeps its own bounce.
const MAX_OVERSHOOT: f32 = 0.5;

/// A rectangle as its four corners, clockwise from the top left, in cell coordinates.
pub(crate) type Corners = [Vec2; 4];

/// The caret's rectangle at `(x, y)`, shaped like the caret.
pub(crate) fn caret_rect(x: u16, y: u16, shape: crate::CaretShape) -> Corners {
    let (x, y) = (f32::from(x), f32::from(y));
    let (left, top, right, bottom) = match shape {
        crate::CaretShape::Bar => (x, y, x + 1.0 / 8.0, y + 1.0),
        crate::CaretShape::Underline => (x, y + 7.0 / 8.0, x + 1.0, y + 1.0),
        crate::CaretShape::Block | crate::CaretShape::TerminalDefault => (x, y, x + 1.0, y + 1.0),
    };
    [
        Vec2::new(left, top),
        Vec2::new(right, top),
        Vec2::new(right, bottom),
        Vec2::new(left, bottom),
    ]
}

pub(crate) fn center(corners: &Corners) -> Vec2 {
    corners
        .iter()
        .fold(Vec2::ZERO, |sum, corner| sum + *corner)
        .scale(0.25)
}

/// `corners`, each pulled to within `max_length` of its target; zero or less leaves them.
fn clamp_to(corners: Corners, target: &Corners, max_length: f32) -> Corners {
    if max_length <= 0.0 {
        return corners;
    }
    std::array::from_fn(|index| {
        let offset = corners[index] - target[index];
        let length = offset.length();
        if length > max_length {
            target[index] + offset.scale(max_length / length)
        } else {
            corners[index]
        }
    })
}

/// The unit direction from one rectangle's centre to another's, or zero for no move.
fn direction(from: &Corners, to: &Corners) -> Vec2 {
    let along = center(to) - center(from);
    let length = along.length();
    if length < f32::EPSILON {
        Vec2::ZERO
    } else {
        along.scale(1.0 / length)
    }
}

/// How far each corner leads the motion, from 0 (trailing) to 1 (leading).
fn leadership(from: &Corners, to: &Corners) -> [f32; 4] {
    let direction = center(to) - center(from);
    let length = direction.length();
    if length < f32::EPSILON {
        return [1.0; 4];
    }
    let direction = direction.scale(1.0 / length);
    let target_center = center(to);
    let projections = to.map(|corner| (corner - target_center).dot(direction));
    let min = projections.iter().copied().fold(f32::INFINITY, f32::min);
    let max = projections
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    if (max - min).abs() < f32::EPSILON {
        return [1.0; 4];
    }
    projections.map(|projection| (projection - min) / (max - min))
}

/// A smear in flight.
#[derive(Clone, Debug)]
pub(crate) struct Smear {
    target: Corners,
    current: Corners,
    previous: Corners,
    velocity: Corners,
    /// Per-corner spring constants (Spring) or progress offsets (Glide), from the corners'
    /// leadership toward the current target.
    lead: [f32; 4],
    /// Unit direction of travel toward the current target, for the overshoot cap.
    direction: Vec2,
    /// Time integrated but not yet a whole step: the interpolation fraction between `previous`
    /// and `current`.
    pending: Duration,
    /// Glide only: where the corners started and how long they have been moving.
    glide_from: Corners,
    glide_elapsed: Duration,
    settled: bool,
}

impl Smear {
    /// A smear that leaves `from` for `to`, starting no more than `max_length` cells behind it so
    /// a cap never pulls the tail forward mid-flight.
    pub(crate) fn new(from: Corners, to: Corners, max_length: f32) -> Self {
        let from = clamp_to(from, &to, max_length);
        Self {
            lead: leadership(&from, &to),
            direction: direction(&from, &to),
            target: to,
            current: from,
            previous: from,
            velocity: [Vec2::ZERO; 4],
            pending: Duration::ZERO,
            glide_from: from,
            glide_elapsed: Duration::ZERO,
            settled: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(from: Corners, to: Corners) -> Self {
        Self::new(from, to, 0.0)
    }

    /// Send the smear somewhere else mid-flight. It keeps its shape and its momentum.
    pub(crate) fn retarget(&mut self, to: Corners) {
        if to == self.target {
            return;
        }
        let shown = self.shown();
        self.lead = leadership(&shown, &to);
        self.direction = direction(&shown, &to);
        self.glide_from = shown;
        self.glide_elapsed = Duration::ZERO;
        self.target = to;
        self.settled = false;
    }

    pub(crate) fn target(&self) -> &Corners {
        &self.target
    }

    pub(crate) fn settled(&self) -> bool {
        self.settled
    }

    /// The corners at the last two physics steps. Their hull is what the smear swept between
    /// them, so a frame drawn from it always reaches back to the one before: a fast smear reads
    /// as one streak rather than a shape hopping from place to place.
    pub(crate) fn swept(&self) -> [Vec2; 8] {
        std::array::from_fn(|index| {
            if index < 4 {
                self.previous[index]
            } else {
                self.current[index - 4]
            }
        })
    }

    /// The centre of the smear as of the latest physics step.
    pub(crate) fn current_center(&self) -> Vec2 {
        center(&self.current)
    }

    /// The corners as of the last advance: between the last two physics steps, so a frame
    /// between steps is smooth at the cost of trailing the integration by up to one step.
    pub(crate) fn shown(&self) -> Corners {
        let alpha = self.pending.as_secs_f32() / STEP.as_secs_f32();
        std::array::from_fn(|index| self.previous[index].lerp(self.current[index], alpha))
    }

    /// Velocity of the smear's centre, in cells per second.
    pub(crate) fn velocity(&self) -> Vec2 {
        center(&self.velocity).scale(1.0 / STEP.as_secs_f32())
    }

    /// Integrate `dt`, calling `on_step` after each whole physics step, for anything that is
    /// driven by the smear (particles).
    pub(crate) fn advance(
        &mut self,
        dt: Duration,
        motion: CursorTrailMotion,
        max_length: f32,
        mut on_step: impl FnMut(&Smear),
    ) {
        if self.settled {
            return;
        }
        self.pending += dt;
        let mut steps = 0;
        while self.pending >= STEP {
            self.pending -= STEP;
            steps += 1;
            if steps > MAX_STEPS_PER_ADVANCE {
                self.settle();
                return;
            }
            self.previous = self.current;
            match motion {
                CursorTrailMotion::Spring(spring) => self.spring_step(spring),
                CursorTrailMotion::Glide { duration } => self.glide_step(duration),
            }
            self.clamp_length(max_length);
            on_step(self);
            if self.settled {
                return;
            }
        }
    }

    fn spring_step(&mut self, spring: CursorTrailSpring) {
        let exponent = spring.trailing_exponent.max(f32::EPSILON);
        let keep = (1.0 - spring.damping).clamp(0.0, 1.0);
        let mut at_rest = true;
        for index in 0..4 {
            let stiffness = spring.trailing_stiffness
                + (spring.stiffness - spring.trailing_stiffness) * self.lead[index].powf(exponent);
            let pull = (self.target[index] - self.current[index]).scale(stiffness);
            self.velocity[index] = self.velocity[index] + pull;
            self.current[index] = self.current[index] + self.velocity[index];
            self.velocity[index] = self.velocity[index].scale(keep);
            let past = (self.current[index] - self.target[index]).dot(self.direction);
            if past > MAX_OVERSHOOT {
                // Pull the corner back to the cap and take away the speed carrying it further.
                self.current[index] =
                    self.current[index] - self.direction.scale(past - MAX_OVERSHOOT);
                let onward = self.velocity[index].dot(self.direction);
                if onward > 0.0 {
                    self.velocity[index] = self.velocity[index] - self.direction.scale(onward);
                }
            }
            at_rest &= (self.target[index] - self.current[index]).length() < spring.stop_distance
                && self.velocity[index].length() < spring.stop_distance;
        }
        if at_rest {
            self.settle();
        }
    }

    fn glide_step(&mut self, duration: Duration) {
        self.glide_elapsed += STEP;
        let progress = if duration.is_zero() {
            1.0
        } else {
            (self.glide_elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };
        let eased = ease_out_cubic(progress);
        for index in 0..4 {
            // Leading corners are on the caret at once; trailing ones take the whole glide.
            let local = eased + (1.0 - eased) * self.lead[index];
            let next = self.glide_from[index].lerp(self.target[index], local);
            self.velocity[index] = next - self.current[index];
            self.current[index] = next;
        }
        if progress >= 1.0 {
            self.settle();
        }
    }

    /// No corner may trail its target by more than `max_length` cells.
    fn clamp_length(&mut self, max_length: f32) {
        self.current = clamp_to(self.current, &self.target, max_length);
    }

    fn settle(&mut self) {
        self.current = self.target;
        self.previous = self.target;
        self.velocity = [Vec2::ZERO; 4];
        self.pending = Duration::ZERO;
        self.settled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaretShape;

    fn spring() -> CursorTrailMotion {
        CursorTrailMotion::Spring(CursorTrailSpring::default())
    }

    fn run(smear: &mut Smear, motion: CursorTrailMotion, total: Duration, frame: Duration) {
        let mut left = total;
        while !left.is_zero() {
            let dt = frame.min(left);
            smear.advance(dt, motion, 25.0, |_| {});
            left -= dt;
        }
    }

    #[test]
    fn a_spring_smear_settles_onto_the_caret() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(30, 4, CaretShape::Block),
        );
        run(&mut smear, spring(), Duration::from_secs(2), STEP);
        assert!(smear.settled());
        assert_eq!(smear.shown(), caret_rect(30, 4, CaretShape::Block));
    }

    /// The default spring is a smear you can see: on a long jump it stretches well past the
    /// caret's own size and takes a good number of frames to shrink back, where smear-cursor's
    /// preset is over in a few.
    #[test]
    fn the_default_spring_stretches_and_lingers() {
        let run = |spring: CursorTrailSpring| {
            let mut smear = Smear::new_for_tests(
                caret_rect(0, 0, CaretShape::Block),
                caret_rect(60, 0, CaretShape::Block),
            );
            let (mut longest, mut overshoot, mut frames) = (0.0f32, 0.0f32, 0);
            while !smear.settled() {
                smear.advance(STEP, CursorTrailMotion::Spring(spring), 25.0, |_| {});
                let shown = smear.shown();
                let xs = shown.iter().map(|corner| corner.x);
                let right = xs.clone().fold(f32::MIN, f32::max);
                longest = longest.max(right - xs.fold(f32::MAX, f32::min));
                // The caret's right edge is column 61.
                overshoot = overshoot.max(right - 61.0);
                frames += 1;
                assert!(frames < 120, "never settled");
            }
            (longest, overshoot, frames)
        };
        let (longest, overshoot, frames) = run(CursorTrailSpring::DEFAULT);
        assert!(longest >= 10.0, "stretches to {longest} cells");
        assert!(frames >= 8, "visible for {frames} frames");
        assert!(
            overshoot <= MAX_OVERSHOOT + 1e-3,
            "the front lands without flying onto the text past the caret: {overshoot} cells"
        );
        let (_, _, quick) = run(CursorTrailSpring::SMEAR_CURSOR);
        assert!(
            quick < frames,
            "smear-cursor's preset is the quicker one: {quick} vs {frames}"
        );
    }

    #[test]
    fn leading_corners_outrun_trailing_ones() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(20, 0, CaretShape::Block),
        );
        smear.advance(STEP * 3, spring(), 25.0, |_| {});
        let shown = smear.shown();
        // Moving right: the right-hand corners (1, 2) lead.
        assert!(
            shown[1].x > shown[0].x + 1.5,
            "the smear stretches: {shown:?}"
        );
        assert!(
            shown[2].x > shown[3].x + 1.5,
            "the smear stretches: {shown:?}"
        );
    }

    #[test]
    fn motion_does_not_depend_on_the_frame_rate() {
        for motion in [
            spring(),
            CursorTrailMotion::Glide {
                duration: Duration::from_millis(160),
            },
        ] {
            let at = |frame: Duration| {
                let mut smear = Smear::new_for_tests(
                    caret_rect(0, 0, CaretShape::Block),
                    caret_rect(40, 10, CaretShape::Block),
                );
                run(&mut smear, motion, Duration::from_millis(100), frame);
                smear.shown()
            };
            let slow = at(Duration::from_millis(33));
            let fast = at(Duration::from_nanos(6_944_444));
            for (slow, fast) in slow.iter().zip(fast.iter()) {
                assert!(
                    (*slow - *fast).length() < 0.75,
                    "{motion:?}: 30 fps {slow:?} vs 144 fps {fast:?}"
                );
            }
        }
    }

    /// A capped smear starts within its cap, so the cap never pulls the tail forward mid-flight.
    #[test]
    fn a_length_cap_holds_from_the_first_frame() {
        let target = caret_rect(100, 0, CaretShape::Block);
        let smear = Smear::new(caret_rect(0, 0, CaretShape::Block), target, 10.0);
        for (corner, target) in smear.shown().iter().zip(target.iter()) {
            assert!((*corner - *target).length() <= 10.0 + 1e-3);
        }
    }

    #[test]
    fn no_corner_trails_further_than_the_maximum_length() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(100, 0, CaretShape::Block),
        );
        // Shown corners are a step behind the integration, so look after two.
        smear.advance(STEP * 2, spring(), 10.0, |_| {});
        for (corner, target) in smear.shown().iter().zip(smear.target().iter()) {
            assert!((*corner - *target).length() <= 10.0 + 1e-3);
        }
    }

    #[test]
    fn a_retarget_keeps_the_momentum() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(20, 0, CaretShape::Block),
        );
        smear.advance(STEP * 2, spring(), 25.0, |_| {});
        let before = smear.shown();
        smear.retarget(caret_rect(20, 10, CaretShape::Block));
        assert_eq!(smear.shown(), before, "no jump on retarget");
        smear.advance(STEP, spring(), 25.0, |_| {});
        assert!(smear.velocity().x > 0.0, "still travelling right");
        run(&mut smear, spring(), Duration::from_secs(2), STEP);
        assert_eq!(smear.shown(), caret_rect(20, 10, CaretShape::Block));
    }

    #[test]
    fn a_glide_puts_the_head_on_the_caret_at_once() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(20, 0, CaretShape::Block),
        );
        let glide = CursorTrailMotion::Glide {
            duration: Duration::from_millis(160),
        };
        smear.advance(STEP * 2, glide, 25.0, |_| {});
        let shown = smear.shown();
        let target = caret_rect(20, 0, CaretShape::Block);
        assert!(
            (shown[1] - target[1]).length() < 1e-3,
            "leading corner on the caret"
        );
        assert!(
            shown[0].x < 10.0,
            "trailing corner still far behind: {shown:?}"
        );
        run(&mut smear, glide, Duration::from_millis(200), STEP);
        assert!(smear.settled());
    }

    #[test]
    fn caret_shapes_give_their_own_rectangles() {
        let bar = caret_rect(3, 2, CaretShape::Bar);
        assert!((bar[1].x - bar[0].x - 0.125).abs() < 1e-6);
        let underline = caret_rect(3, 2, CaretShape::Underline);
        assert!((underline[3].y - underline[0].y - 0.125).abs() < 1e-6);
    }
}
