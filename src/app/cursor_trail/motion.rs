//! How the smear moves: a head chasing the caret and a tail chasing the head along its path.
//!
//! The head is the caret's own rectangle. When the caret jumps, a damped spring pulls the head
//! after it, and every physics step adds where the head got to onto a path. The tail follows the
//! head along that path on a slower spring, so the smear stretches out behind the caret and
//! shrinks back into it. What is drawn is a ribbon along the path from the tail to the head, the
//! caret's rectangle narrowing toward the tail.
//!
//! Following the path keeps the smear a ribbon whatever the caret does: sent somewhere else
//! mid-flight, the head swings round with its momentum and the tail follows the bend, so the
//! smear curves rather than turning into a slab. The spring values mean what smear-cursor.nvim's
//! do for the caret's leading and trailing edges: a spring constant and a velocity damping, both
//! applied once per sixtieth of a second.
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
/// The furthest the head may fly past the caret along the direction of travel, in cells. A
/// spring's overshoot grows with the jump, so a long one would otherwise land the smear on text
/// beyond the caret; capped, a long jump still snaps, and a short one keeps its own bounce.
const MAX_OVERSHOOT: f32 = 0.5;
/// Path points closer together than this, in cells, are one point.
const MIN_SEGMENT: f32 = 0.05;
/// How much of the path behind the tail is kept, in cells: more than the head travels between
/// two frames, however slow, so a frame can always reach back to the one before.
const KEPT_BEHIND: f32 = 64.0;

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

/// A rectangle of `size` centred on `centre`, shrunk to `scale` of it.
fn rect_at(centre: Vec2, size: Vec2, scale: f32) -> Corners {
    let half = size.scale(0.5 * scale);
    [
        Vec2::new(centre.x - half.x, centre.y - half.y),
        Vec2::new(centre.x + half.x, centre.y - half.y),
        Vec2::new(centre.x + half.x, centre.y + half.y),
        Vec2::new(centre.x - half.x, centre.y + half.y),
    ]
}

fn unit(vector: Vec2) -> Vec2 {
    let length = vector.length();
    if length < f32::EPSILON {
        Vec2::ZERO
    } else {
        vector.scale(1.0 / length)
    }
}

/// How wide the smear's tail is next to its head, from the spring's trailing exponent: higher
/// keeps the tail fuller, as smear-cursor's does with its trailing corners.
fn tail_width(spring: Option<CursorTrailSpring>) -> f32 {
    let exponent = spring
        .map_or(CursorTrailSpring::DEFAULT.trailing_exponent, |spring| {
            spring.trailing_exponent
        })
        .max(0.0);
    exponent / (1.0 + exponent)
}

/// One frame of the smear, as pieces to draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ribbon {
    /// Each piece is the corners of the caret's rectangle at two neighbouring points of the path:
    /// their convex hull is that stretch of the ribbon.
    pub(crate) pieces: Vec<[Vec2; 8]>,
    /// The centre of the ribbon's tail end.
    pub(crate) tail: Vec2,
    /// How far along the path the head is, for the next frame to reach back to.
    pub(crate) head_arc: f32,
}

/// A smear in flight.
#[derive(Clone, Debug)]
pub(crate) struct Smear {
    target: Corners,
    /// The caret's size, which the ribbon is as wide as.
    size: Vec2,
    /// The head's centre at the latest physics step and at the one before, and its velocity.
    head: Vec2,
    head_before: Vec2,
    velocity: Vec2,
    /// Where the head has been, oldest first, with each point's distance along the path. The last
    /// point is the head at the latest step.
    path: Vec<(Vec2, f32)>,
    /// How far along the path the head was at the step before the latest.
    head_arc_before: f32,
    /// How far along the path the tail is, at the latest step and at the one before.
    tail: f32,
    tail_before: f32,
    tail_velocity: f32,
    /// Unit direction of travel toward the current target, for the overshoot cap.
    direction: Vec2,
    /// Time integrated but not yet a whole step: the interpolation fraction between the last two.
    pending: Duration,
    /// Glide only: where the tail started and how long it has been moving.
    glide_from: f32,
    glide_elapsed: Duration,
    /// How wide the tail is next to the head, from the last spring seen.
    tail_width: f32,
    settled: bool,
}

impl Smear {
    /// A smear that leaves `from` for `to`, starting no more than `max_length` cells behind it so
    /// a cap never pulls the tail forward mid-flight.
    pub(crate) fn new(from: Corners, to: Corners, max_length: f32) -> Self {
        let target = center(&to);
        let mut start = center(&from);
        let offset = start - target;
        if max_length > 0.0 && offset.length() > max_length {
            start = target + offset.scale(max_length / offset.length());
        }
        Self {
            target: to,
            size: to[2] - to[0],
            head: start,
            head_before: start,
            velocity: Vec2::ZERO,
            path: vec![(start, 0.0)],
            head_arc_before: 0.0,
            tail: 0.0,
            tail_before: 0.0,
            tail_velocity: 0.0,
            direction: unit(target - start),
            pending: Duration::ZERO,
            glide_from: 0.0,
            glide_elapsed: Duration::ZERO,
            tail_width: tail_width(None),
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
        self.size = to[2] - to[0];
        self.direction = unit(center(&to) - self.head);
        self.glide_from = self.tail;
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

    /// The centre of the head as of the latest physics step.
    pub(crate) fn current_center(&self) -> Vec2 {
        self.head
    }

    /// Velocity of the head, in cells per second.
    pub(crate) fn velocity(&self) -> Vec2 {
        self.velocity.scale(1.0 / STEP.as_secs_f32())
    }

    fn alpha(&self) -> f32 {
        self.pending.as_secs_f32() / STEP.as_secs_f32()
    }

    fn head_arc(&self) -> f32 {
        self.path.last().map_or(0.0, |&(_, arc)| arc)
    }

    /// The point `arc` along the path.
    fn point_at(&self, arc: f32) -> Vec2 {
        let index = self.path.partition_point(|&(_, at)| at <= arc);
        match (
            index.checked_sub(1).map(|i| self.path[i]),
            self.path.get(index),
        ) {
            (Some((a, at_a)), Some(&(b, at_b))) if at_b > at_a => {
                a.lerp(b, (arc - at_a) / (at_b - at_a))
            }
            (Some((a, _)), _) => a,
            (None, Some(&(b, _))) => b,
            (None, None) => self.head,
        }
    }

    /// The caret's rectangle where the head is as of the last advance.
    #[cfg(test)]
    pub(crate) fn shown_head(&self) -> Corners {
        rect_at(
            self.head_before.lerp(self.head, self.alpha()),
            self.size,
            1.0,
        )
    }

    /// The smear as of the last advance, between the last two physics steps, so a frame between
    /// steps is smooth at the cost of trailing the integration by up to one step. It reaches back
    /// along the path to `reach_back` (where the last drawn frame's head was) if its tail has
    /// already passed that, so consecutive frames always touch and a fast smear reads as one
    /// streak rather than a shape hopping from place to place.
    pub(crate) fn ribbon(&self, reach_back: Option<f32>) -> Ribbon {
        let alpha = self.alpha();
        let head_arc = self.head_arc_before + (self.head_arc() - self.head_arc_before) * alpha;
        let head = self.head_before.lerp(self.head, alpha);
        let tail_arc = (self.tail_before + (self.tail - self.tail_before) * alpha).min(head_arc);
        let start = reach_back
            .map_or(tail_arc, |arc| tail_arc.min(arc))
            .max(self.path.first().map_or(0.0, |&(_, arc)| arc));
        let mut points = vec![(self.point_at(start), start)];
        points.extend(
            self.path
                .iter()
                .copied()
                .filter(|&(_, arc)| arc > start + MIN_SEGMENT && arc < head_arc - MIN_SEGMENT),
        );
        points.push((head, head_arc));
        let span = head_arc - start;
        let width = |arc: f32| {
            if span < MIN_SEGMENT {
                1.0
            } else {
                self.tail_width + (1.0 - self.tail_width) * ((arc - start) / span)
            }
        };
        let pieces = if points.len() < 2 || span < MIN_SEGMENT {
            let rect = rect_at(head, self.size, 1.0);
            vec![[
                rect[0], rect[1], rect[2], rect[3], rect[0], rect[1], rect[2], rect[3],
            ]]
        } else {
            points
                .windows(2)
                .map(|pair| {
                    let (a, b) = (
                        rect_at(pair[0].0, self.size, width(pair[0].1)),
                        rect_at(pair[1].0, self.size, width(pair[1].1)),
                    );
                    [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]]
                })
                .collect()
        };
        Ribbon {
            pieces,
            tail: points[0].0,
            head_arc,
        }
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
        self.tail_width = tail_width(match motion {
            CursorTrailMotion::Spring(spring) => Some(spring),
            CursorTrailMotion::Glide { .. } => None,
        });
        self.pending += dt;
        let mut steps = 0;
        while self.pending >= STEP {
            self.pending -= STEP;
            steps += 1;
            if steps > MAX_STEPS_PER_ADVANCE {
                self.settle();
                return;
            }
            self.head_before = self.head;
            self.head_arc_before = self.head_arc();
            self.tail_before = self.tail;
            match motion {
                CursorTrailMotion::Spring(spring) => self.spring_step(spring),
                CursorTrailMotion::Glide { duration } => self.glide_step(duration),
            }
            let length = self.head_arc() - self.tail;
            if max_length > 0.0 && length > max_length {
                self.tail = self.head_arc() - max_length;
            }
            self.prune();
            on_step(self);
            if self.settled {
                return;
            }
        }
    }

    /// Move the head to `to`, extending the path.
    fn move_head(&mut self, to: Vec2) {
        let (last, arc) = self.path.last().copied().unwrap_or((self.head, 0.0));
        let step = (to - last).length();
        if step >= MIN_SEGMENT {
            self.path.push((to, arc + step));
        } else if let Some(point) = self.path.last_mut() {
            // Too short a step for a point of its own: the head moves, the path keeps its shape.
            point.0 = to;
        }
        self.head = to;
    }

    fn spring_step(&mut self, spring: CursorTrailSpring) {
        let keep = (1.0 - spring.damping).clamp(0.0, 1.0);
        let target = center(&self.target);
        let mut velocity = self.velocity + (target - self.head).scale(spring.stiffness);
        let mut head = self.head + velocity;
        let past = (head - target).dot(self.direction);
        if past > MAX_OVERSHOOT {
            // Pull the head back to the cap and take away the speed carrying it further.
            head = head - self.direction.scale(past - MAX_OVERSHOOT);
            let onward = velocity.dot(self.direction);
            if onward > 0.0 {
                velocity = velocity - self.direction.scale(onward);
            }
        }
        self.velocity = velocity.scale(keep);
        self.move_head(head);

        let behind = self.head_arc() - self.tail;
        self.tail_velocity += behind * spring.trailing_stiffness;
        self.tail += self.tail_velocity;
        self.tail_velocity *= keep;
        if self.tail > self.head_arc() {
            self.tail = self.head_arc();
            self.tail_velocity = 0.0;
        }

        let at_rest = (target - self.head).length() < spring.stop_distance
            && self.velocity.length() < spring.stop_distance
            && self.head_arc() - self.tail < spring.stop_distance;
        if at_rest {
            self.settle();
        }
    }

    fn glide_step(&mut self, duration: Duration) {
        // The head is on the caret at once; the tail takes the whole glide.
        self.velocity = Vec2::ZERO;
        self.move_head(center(&self.target));
        self.glide_elapsed += STEP;
        let progress = if duration.is_zero() {
            1.0
        } else {
            (self.glide_elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };
        self.tail =
            self.glide_from + (self.head_arc() - self.glide_from) * ease_out_cubic(progress);
        if progress >= 1.0 {
            self.settle();
        }
    }

    /// Drop the path more than [`KEPT_BEHIND`] behind the tail, but for the point that lies on.
    /// The stretch kept is what a frame may reach back into, to touch the frame before it.
    fn prune(&mut self) {
        let keep_from = self.tail.min(self.tail_before) - KEPT_BEHIND;
        let behind = self.path.partition_point(|&(_, arc)| arc <= keep_from);
        if behind > 1 {
            self.path.drain(..behind - 1);
        }
    }

    fn settle(&mut self) {
        let target = center(&self.target);
        let arc = self.head_arc() + (target - self.head).length();
        self.head = target;
        self.head_before = target;
        self.velocity = Vec2::ZERO;
        self.path = vec![(target, arc)];
        self.head_arc_before = arc;
        self.tail = arc;
        self.tail_before = arc;
        self.tail_velocity = 0.0;
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

    /// Every corner of every piece of the ribbon.
    fn points(ribbon: &Ribbon) -> Vec<Vec2> {
        ribbon.pieces.iter().flatten().copied().collect()
    }

    #[test]
    fn a_spring_smear_settles_onto_the_caret() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(30, 4, CaretShape::Block),
        );
        run(&mut smear, spring(), Duration::from_secs(2), STEP);
        assert!(smear.settled());
        assert_eq!(smear.shown_head(), caret_rect(30, 4, CaretShape::Block));
        let ribbon = smear.ribbon(None);
        assert_eq!(ribbon.pieces.len(), 1, "only the caret is left");
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
                let points = points(&smear.ribbon(None));
                let xs = points.iter().map(|corner| corner.x);
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
        assert!(longest >= 8.0, "stretches to {longest} cells");
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

    /// The smear narrows toward its tail, so a diagonal one is a tapering streak rather than a
    /// band the caret's full height.
    #[test]
    fn a_smear_tapers_to_its_tail() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(60, 20, CaretShape::Block),
        );
        smear.advance(STEP * 3, spring(), 0.0, |_| {});
        let ribbon = smear.ribbon(None);
        let size = |rect: &[Vec2]| (rect[2] - rect[0]).length();
        let tail_end = &ribbon.pieces[0][..4];
        let head_end = &ribbon.pieces[ribbon.pieces.len() - 1][4..];
        assert!(
            size(tail_end) * 2.0 < size(head_end),
            "the tail {tail_end:?} is narrower than the head {head_end:?}"
        );
    }

    #[test]
    fn the_head_outruns_the_tail() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(20, 0, CaretShape::Block),
        );
        smear.advance(STEP * 3, spring(), 25.0, |_| {});
        let ribbon = smear.ribbon(None);
        let head = smear.shown_head();
        assert!(
            head[0].x > ribbon.tail.x + 1.5,
            "the smear stretches: tail {:?}, head {head:?}",
            ribbon.tail
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
                (smear.shown_head(), smear.ribbon(None).tail)
            };
            let (slow_head, slow_tail) = at(Duration::from_millis(33));
            let (fast_head, fast_tail) = at(Duration::from_nanos(6_944_444));
            for (slow, fast) in [(slow_head[0], fast_head[0]), (slow_tail, fast_tail)] {
                assert!(
                    (slow - fast).length() < 0.75,
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
        for point in points(&smear.ribbon(None)) {
            assert!((point - center(&target)).length() <= 10.0 + 1.0);
        }
    }

    #[test]
    fn the_tail_trails_no_further_than_the_maximum_length() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(100, 0, CaretShape::Block),
        );
        for _ in 0..6 {
            smear.advance(STEP, spring(), 10.0, |_| {});
            let ribbon = smear.ribbon(None);
            let head = center(&smear.shown_head());
            assert!(
                (head - ribbon.tail).length() <= 10.0 + 1e-3,
                "tail {:?} head {head:?}",
                ribbon.tail
            );
        }
    }

    #[test]
    fn a_retarget_keeps_the_momentum() {
        let mut smear = Smear::new_for_tests(
            caret_rect(0, 0, CaretShape::Block),
            caret_rect(20, 0, CaretShape::Block),
        );
        smear.advance(STEP * 2, spring(), 25.0, |_| {});
        let before = smear.ribbon(None);
        smear.retarget(caret_rect(20, 10, CaretShape::Block));
        assert_eq!(smear.ribbon(None), before, "no jump on retarget");
        smear.advance(STEP, spring(), 25.0, |_| {});
        assert!(smear.velocity().x > 0.0, "still travelling right");
        run(&mut smear, spring(), Duration::from_secs(2), STEP);
        assert_eq!(smear.shown_head(), caret_rect(20, 10, CaretShape::Block));
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
        let target = caret_rect(20, 0, CaretShape::Block);
        assert!(
            (smear.shown_head()[0] - target[0]).length() < 1e-3,
            "head on the caret"
        );
        let tail = smear.ribbon(None).tail;
        assert!(tail.x < 10.0, "tail still far behind: {tail:?}");
        run(&mut smear, glide, Duration::from_millis(200), STEP);
        assert!(smear.settled());
    }

    /// The ribbon follows the head's path: sent off sideways mid-flight, the smear bends along
    /// the way the head went instead of cutting the corner.
    #[test]
    fn a_smear_sent_elsewhere_follows_the_bend() {
        let mut smear = Smear::new_for_tests(
            caret_rect(40, 0, CaretShape::Block),
            caret_rect(40, 30, CaretShape::Block),
        );
        smear.advance(STEP * 3, spring(), 0.0, |_| {});
        smear.retarget(caret_rect(0, 30, CaretShape::Block));
        smear.advance(STEP * 2, spring(), 0.0, |_| {});
        let ribbon = smear.ribbon(None);
        assert!(ribbon.pieces.len() >= 2, "{} pieces", ribbon.pieces.len());
        // Every piece hugs the path: no wider than the caret's diagonal.
        for piece in &ribbon.pieces {
            let (a, b) = (
                center(&[piece[0], piece[1], piece[2], piece[3]]),
                center(&[piece[4], piece[5], piece[6], piece[7]]),
            );
            let along = unit(b - a);
            let across = Vec2::new(-along.y, along.x);
            let spread = piece.iter().map(|point| (*point - a).dot(across));
            let width = spread.clone().fold(f32::MIN, f32::max) - spread.fold(f32::MAX, f32::min);
            assert!(
                width <= 2f32.sqrt() + 1e-3,
                "a piece {width} cells wide: {piece:?}"
            );
        }
    }

    #[test]
    fn caret_shapes_give_their_own_rectangles() {
        let bar = caret_rect(3, 2, CaretShape::Bar);
        assert!((bar[1].x - bar[0].x - 0.125).abs() < 1e-6);
        let underline = caret_rect(3, 2, CaretShape::Underline);
        assert!((underline[3].y - underline[0].y - 0.125).abs() < 1e-6);
    }
}
