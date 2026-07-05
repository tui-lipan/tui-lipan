# Visual effects (`VisualEffect`)

Declarative post-processing for `EffectScope` and hover passes on `MouseRegion` (`hover_effect` / `hover_effects`).

## `VisualEffect::Gradient`

Uses `ColorGradient` stops (`min` -> optional `center` -> `max`) sampled along `EffectAxis` in **scope-local** normalized coordinates (nested scopes remap independently), then blended onto rendered fg/bg like `RainbowWave`. `frequency` repeats a sine-eased mirrored ramp (`min -> max -> min`) across the scope, avoiding hard wrap seams and sharp endpoint troughs; `speed` shifts the pattern using the renderer phase (`0.0` = static).

## `VisualEffect::Ripple`

`Ripple` uses `origin: EffectOrigin`, so the ring can be pinned to explicit scope-local cells or resolved from the current `EffectScope` bounds at render time. Its `radius: RippleRadius` is the animation knob: `Fixed` is static, `Loop` repeats from zero to `max_radius`, and `Once` plays a single burst from a captured renderer `start_tick`.

```rust
VisualEffect::Ripple {
    origin: EffectOrigin::cell(12.0, 3.0),
    radius: RippleRadius::Fixed(4.0),
    ring_width: 1.5,
    tint: Color::Cyan,
    strength: 0.6,
}

VisualEffect::centered_ripple(4.0, 1.5, Color::Cyan, 0.6)

VisualEffect::centered_looping_ripple(18.0, 90, 1.5, Color::Cyan, 0.6)

let start_tick = ctx.effect_phase();
VisualEffect::centered_burst_ripple(18.0, 45, start_tick, 1.5, Color::Cyan, 0.6)

VisualEffect::Ripple {
    origin: EffectOrigin::aligned(EffectAlignment::TOP_RIGHT),
    radius: RippleRadius::Once {
        max_radius: 18.0,
        duration_ticks: 45,
        start_tick,
    },
    ring_width: 1.5,
    tint: Color::Cyan,
    strength: 0.6,
}
```

`Loop` and `Once` automatically mark the effect as animated so the runtime schedules repaint ticks. Radius growth uses ease-out (`1 - (1 - t)^2`); strength fades linearly by `1 - t`. `Once` stops rendering outside its window, but callers should remove the effect or replace it with `Fixed` after completion so the animation ticker can go idle.

## `VisualEffect::Clipped`

Restricts an inner effect to a sub-rectangle of the scope and/or a per-cell bitmask:

- `bounds: Option<Rect>` - clip rect in **scope-local** coordinates (origin at the effect scope’s top-left). `None` means the full scope.
- `mask: Option<Arc<CellMask>>` - optional bitmap; cells where the mask is false skip the inner effect. `None` means a solid rectangle (`bounds`, or the full scope when `bounds` is `None`).
- `inner` - another `VisualEffect` (for example `Ripple` or `Dim`).

`CellMask` stores `origin`, `w`, `h`, and row-major packed bits in `Arc<[u64]>`. Use `CellMask::test_scope_local` for scope-local coordinates.

`BigText::layout_glyphs` builds the same raster as `BigText::build_lines()` for each line of text, splits it into per-character column bands, then derives each glyph’s ink `Rect` and `CellMask`. **Exact** letter boundaries only when FIGlet leaves at least one fully blank column between glyphs; when letters touch, bands use each character’s **standalone FIGlet width** (same font and style as the line), scaled to the full ink span - closer than equal slices, though smushed strings can still differ slightly from per-glyph truth. Blank lines advance the vertical offset by one row each; `"A\n\nB"` inserts a single empty row between blocks. Use a single `MouseRegion` over the `BigText` with `hit_test` / pointer move using those masks so coordinates stay in the shared scope. See the “Letter burst” tab in `examples/burst_effects.rs`.

Nested `Clipped` layers compose; each layer’s `bounds` / `mask` uses the same scope-local coordinate system as the enclosing `EffectScope`.

## Custom Effects

`VisualEffect::Custom(Arc<dyn CellEffect>)` lets applications add their own per-cell post-processing pass without forking the renderer. The effect receives an `EffectCell` plus an `EffectContext` containing the absolute cell position, the absolute effect-scope bounds, the animation `phase`, and the host terminal background when known.

For expensive effects, override `prepare(&EffectPrepareContext)` and return a `PreparedCellEffect`. Preparation runs once per effect scope, bounds, and render phase before the per-cell pass, so you can cache light positions, palettes, masks, or other frame-constant state instead of recomputing it for every cell.

```rust
use std::fmt;

use tui_lipan::prelude::*;

#[derive(Clone)]
struct Vortex {
    strength: f32,
}

impl fmt::Debug for Vortex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vortex")
            .field("strength", &self.strength)
            .finish()
    }
}

impl CellEffect for Vortex {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;
        let cx = ctx.bounds.w as f32 * 0.5;
        let cy = ctx.bounds.h as f32 * 0.5;
        let dx = lx - cx;
        let dy = (ly - cy) * 2.0;
        let radius = (dx * dx + dy * dy).sqrt().max(1.0);
        let angle = dy.atan2(dx) + ctx.phase as f32 * 0.08;
        let wave = ((angle * 3.0 + radius * 0.25).sin() * 0.5 + 0.5) * self.strength;

        if wave > 0.6 {
            cell.set_fg(TerminalColor::Cyan);
        } else if wave > 0.3 {
            cell.set_fg(TerminalColor::Blue);
        }
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn cache_key(&self) -> u64 {
        self.strength.to_bits() as u64
    }
}

#[derive(Debug)]
struct PreparedSpotlight {
    cx: f32,
    cy: f32,
}

impl PreparedCellEffect for PreparedSpotlight {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;
        let dx = lx - self.cx;
        let dy = (ly - self.cy) * 2.0;
        if dx * dx + dy * dy < 64.0 {
            cell.set_fg(TerminalColor::White);
        }
    }
}

#[derive(Clone, Debug)]
struct Spotlight;

impl CellEffect for Spotlight {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        PreparedSpotlight {
            cx: ctx.bounds.w as f32 * 0.5,
            cy: ctx.bounds.h as f32 * 0.5,
        }
        .apply(cell, ctx);
    }

    fn prepare(&self, ctx: &EffectPrepareContext) -> Option<Box<dyn PreparedCellEffect>> {
        Some(Box::new(PreparedSpotlight {
            cx: ctx.bounds.w as f32 * 0.5,
            cy: ctx.bounds.h as f32 * 0.5,
        }))
    }
}

let view = EffectScope::new()
    .custom_effect(Vortex { strength: 0.8 })
    .child(Text::new("custom effect target"));
```

Use `is_animated()` when the effect depends on `ctx.phase` so the runtime schedules redraws. Override `animation_interval()` when an animated custom effect should update below the default ~60 FPS cadence, for example a slow atmospheric glow that looks the same at 30 FPS. Override `cache_key()` when changing effect parameters should invalidate layout/render hashes; the default `0` is safe because custom effect hashes also include `Arc` identity. Custom effect equality uses `Arc` identity too, so two different `Arc`s with the same cache key are not equal.
