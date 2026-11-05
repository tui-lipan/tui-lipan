//! Sparks thrown off by a moving smear.
//!
//! Each particle is a braille dot, so four rows and two columns of them fit in one cell. They are
//! emitted in proportion to how far the smear travels, inherit some of its velocity, scatter at
//! random, fall under gravity, slow under drag, and fade out over their lifetime. The random
//! source is seeded, so a given jump throws the same sparks every time.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::{Color as RColor, Modifier};

use super::raster::{SmearPaint, mix, rgb, under};
use super::{CursorTrailParticles, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
    age: f32,
    lifetime: f32,
}

/// The live particles and the generator that places new ones.
#[derive(Clone, Debug)]
pub(crate) struct Sparks {
    particles: Vec<Particle>,
    seed: u64,
    /// Fractional particles owed from earlier steps, so a slow smear still emits.
    owed: f32,
}

impl Default for Sparks {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            seed: 0x9E37_79B9_7F4A_7C15,
            owed: 0.0,
        }
    }
}

/// A spark as one frame draws it: where it is, and how much of its life is left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spark {
    pub(crate) position: Vec2,
    pub(crate) life: f32,
}

impl Sparks {
    fn random(&mut self) -> f32 {
        // xorshift64*
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let bits = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        bits as f32 / (1u64 << 24) as f32
    }

    pub(crate) fn alive(&self) -> bool {
        !self.particles.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.particles.clear();
        self.owed = 0.0;
    }

    /// Emit for one physics step in which the smear's head moved `travelled` cells. New sparks
    /// start on the smear's body: `body(t)` is the point a fraction `t` of the way along it, from
    /// the tail at 0 to the head at 1, so they follow its bends.
    pub(crate) fn emit(
        &mut self,
        config: &CursorTrailParticles,
        body: impl Fn(f32) -> Vec2,
        velocity: Vec2,
        travelled: f32,
    ) {
        self.owed += travelled * config.per_cell.max(0.0);
        let max = usize::from(config.max);
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            if self.particles.len() >= max {
                continue;
            }
            let along = self.random();
            let angle = self.random() * std::f32::consts::TAU;
            let speed = config.spread * (0.25 + 0.75 * self.random());
            let jitter = Vec2::new(self.random() - 0.5, self.random() - 0.5);
            let lifetime = config.lifetime.as_secs_f32() * (0.6 + 0.4 * self.random());
            self.particles.push(Particle {
                position: body(along) + jitter,
                velocity: velocity.scale(config.inherit)
                    + Vec2::new(angle.cos(), angle.sin()).scale(speed),
                age: 0.0,
                lifetime,
            });
        }
    }

    /// Move every particle on by `dt`, and drop the ones whose life is over.
    pub(crate) fn step(&mut self, config: &CursorTrailParticles, dt: Duration) {
        let dt = dt.as_secs_f32();
        let drag = (-config.drag.max(0.0) * dt).exp();
        for particle in &mut self.particles {
            particle.velocity.y += config.gravity * dt;
            particle.velocity = particle.velocity.scale(drag);
            particle.position = particle.position + particle.velocity.scale(dt);
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);
    }

    pub(crate) fn snapshot(&self) -> Vec<Spark> {
        self.particles
            .iter()
            .map(|particle| Spark {
                position: particle.position,
                life: 1.0 - particle.age / particle.lifetime,
            })
            .collect()
    }
}

/// Braille dot bit for a sub-cell column (0..2) and row (0..4).
fn braille_bit(column: u32, row: u32) -> u32 {
    match (column, row) {
        (0, 3) => 0x40,
        (1, 3) => 0x80,
        (0, row) => 1 << row,
        (_, row) => 1 << (row + 3),
    }
}

/// Draw the sparks as braille dots, leaving cells the smear itself covers to the smear.
pub(crate) fn paint(
    sparks: &[Spark],
    buffer: &mut Buffer,
    color: RColor,
    smear: Option<&SmearPaint>,
    caret: Option<Position>,
    host_bg: Option<(u8, u8, u8)>,
) {
    let area = buffer.area;
    let mut cells: Vec<(Position, u32, f32)> = Vec::new();
    for spark in sparks {
        let (x, y) = (spark.position.x, spark.position.y);
        if x < f32::from(area.left())
            || y < f32::from(area.top())
            || x >= f32::from(area.right())
            || y >= f32::from(area.bottom())
        {
            continue;
        }
        let position = Position::new(x as u16, y as u16);
        if caret == Some(position) || smear.is_some_and(|smear| smear.covers(position)) {
            continue;
        }
        let bit = braille_bit(
            ((x.fract()) * 2.0).min(1.0) as u32,
            ((y.fract()) * 4.0).min(3.0) as u32,
        );
        match cells.iter_mut().find(|(at, ..)| *at == position) {
            Some((_, bits, life)) => {
                *bits |= bit;
                *life = life.max(spark.life);
            }
            None => cells.push((position, bit, spark.life)),
        }
    }
    let trail_rgb = rgb(color);
    for (position, bits, life) in cells {
        let Some(cell) = buffer.cell_mut(position) else {
            continue;
        };
        // Only blank cells and other sparks: a spark never draws over a character, so the text
        // it flies past stays readable.
        let symbol = cell.symbol();
        let blank = symbol.trim().is_empty() && !symbol.is_empty();
        let spark = symbol
            .chars()
            .next()
            .is_some_and(|c| ('\u{2801}'..='\u{28FF}').contains(&c));
        if !blank && !spark {
            continue;
        }
        let fg = match (trail_rgb, under(cell, host_bg)) {
            (Some(trail), Some(under)) => {
                let (r, g, b) = mix(under, trail, life);
                RColor::Rgb(r, g, b)
            }
            _ if life < 0.5 => continue,
            _ => color,
        };
        cell.set_char(char::from_u32(0x2800 + bits).expect("braille code point"));
        cell.modifier = Modifier::empty();
        cell.fg = fg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn config() -> CursorTrailParticles {
        CursorTrailParticles::default()
    }

    #[test]
    fn sparks_are_emitted_by_distance_and_die_after_their_lifetime() {
        let mut sparks = Sparks::default();
        let config = config();
        sparks.emit(
            &config,
            |t| Vec2::new(10.0 * t, 0.0),
            Vec2::new(60.0, 0.0),
            10.0,
        );
        let emitted = sparks.snapshot().len();
        assert_eq!(emitted, (10.0 * config.per_cell) as usize);
        sparks.step(&config, config.lifetime);
        assert!(
            !sparks.alive(),
            "every spark is gone after the longest lifetime"
        );
    }

    #[test]
    fn sparks_fall_under_gravity_and_are_deterministic() {
        let config = CursorTrailParticles {
            spread: 0.0,
            inherit: 0.0,
            ..config()
        };
        let run = || {
            let mut sparks = Sparks::default();
            sparks.emit(&config, |_| Vec2::new(5.0, 5.0), Vec2::ZERO, 4.0);
            let before = sparks.snapshot();
            sparks.step(&config, Duration::from_millis(100));
            (before, sparks.snapshot())
        };
        let (before, after) = run();
        for (before, after) in before.iter().zip(after.iter()) {
            assert!(after.position.y > before.position.y, "gravity pulls down");
            assert!(after.life < before.life);
        }
        assert_eq!(run(), run(), "the same jump throws the same sparks");
    }

    #[test]
    fn the_cap_bounds_the_live_sparks() {
        let config = CursorTrailParticles { max: 5, ..config() };
        let mut sparks = Sparks::default();
        sparks.emit(&config, |t| Vec2::new(50.0 * t, 0.0), Vec2::ZERO, 50.0);
        assert_eq!(sparks.snapshot().len(), 5);
    }

    #[test]
    fn sparks_in_one_cell_share_a_braille_glyph() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        for cell in buffer.content.iter_mut() {
            cell.bg = RColor::Rgb(0, 0, 0);
        }
        let sparks = [
            Spark {
                position: Vec2::new(1.1, 0.1),
                life: 1.0,
            },
            Spark {
                position: Vec2::new(1.9, 0.9),
                life: 0.5,
            },
        ];
        paint(
            &sparks,
            &mut buffer,
            RColor::Rgb(255, 0, 0),
            None,
            None,
            None,
        );
        // Top-left dot (1) and bottom-right dot (0x80).
        assert_eq!(buffer[(1, 0)].symbol(), "\u{2881}");
        assert_eq!(
            buffer[(1, 0)].fg,
            RColor::Rgb(255, 0, 0),
            "the brightest spark"
        );
        assert_eq!(buffer[(0, 0)].symbol(), " ");

        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        paint(
            &sparks,
            &mut buffer,
            RColor::Rgb(255, 0, 0),
            None,
            Some(Position::new(1, 0)),
            None,
        );
        assert_eq!(
            buffer[(1, 0)].symbol(),
            " ",
            "sparks keep off the caret's cell"
        );

        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        buffer[(1, 0)].set_char('t');
        paint(
            &sparks,
            &mut buffer,
            RColor::Rgb(255, 0, 0),
            None,
            None,
            None,
        );
        assert_eq!(buffer[(1, 0)].symbol(), "t", "sparks never draw over text");
    }
}
