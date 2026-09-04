# Incremental terminal repaint: continuation record

Working note for the `perf/terminal-damage-paint` branch, not documentation. Delete or fold into
the merge commit when the work lands.

The problem, measured in Rozi's `docs/performance/audits/2026-09-04-idle-repaint.md`: a
non-fullscreen agent CLI animating a spinner put the client at about 8% of a core. Client CPU is
linear in how often the guest writes *and* in the total cells of the window, and does not move when
the writing pane is shrunk to a quarter of that window. One changed character costs a full-window
frame. Painting is 87% of the draw.

## Proven, and on the branch

Ten commits, each independently verifiable. `cf5537e` .. `5f494c3`.

- **Damage exists and is usable.** `TerminalScreen` folds `Term::damage()` into an accumulator per
  write and `take_damage` hands it to the frame. A nine-column spinner rewrite reports one row.
- **Provenance is positive, not inferred.** `Update::terminal_paint` asserts that live terminal
  content is the only visual change. `DirtyLevel::TerminalPaintOnly` sits below `PaintOnly`, so any
  generic paint, layout or full update in the same frame widens it back, in either order.
- **Damage is consumed once.** `refresh_live_terminals_detailed` returns it; nothing else may ask.
- **Eligibility is decided before execution.** `plan_terminal_damage` is pure and returns a typed
  `DamageRejection`. A `TerminalDamagePlan` means executable, not promising.
- **A one-row clip paints one row.** `render_regions` at a one-row region reproduces that row of a
  full render and leaves every other row unpainted (`4204919`).
- **Selective seeding works.** Seeding only the damaged row from the retained frame into a poisoned
  scratch buffer reproduces a full render on that row and touches nothing else (`3d97c7e`). No
  full-buffer clone, no full-buffer diff.
- **Cursor-only moves report damage.** Alacritty damages the old and new positions, so they arrive
  as ordinary row damage (`5f494c3`).

## Remaining

### 1. Extract the production `RenderContext`

Shared by the ordinary draw and the damage draw. Prefer a scoped
`with_render_context(|ctx| ...)` over returning a `RenderContext` plus its backing locals - the
inline construction owns that temporary storage precisely because the context borrows it.

Invariant: `draw_current_tree_after_live_refresh` produces identical frames before and after, and
changes no eligibility or draw-mode decision. The terminal visual and snapshot tests all run
through it, so the existing suite is the evidence.

### 2. `draw_terminal_damage`

```
save scratch row  ->  seed from last_frame_snapshot  ->  render_regions([row])
row-only diff  ->  backend.draw(updates)
advance last_frame_snapshot        <- only on draw success
restore scratch row                <- unconditional, every exit
cursor sync  ->  flush  ->  no swap_buffers()
```

**The commit point is a successful `backend.draw()`, not a successful function.** This constrains
the code shape, so it cannot be written as "do everything, commit at the end":

- render succeeds but `backend.draw` fails: restore scratch, do **not** advance the snapshot. The
  host may never have received those cells, and a snapshot claiming otherwise poisons every later
  patch.
- `backend.draw` succeeds but cursor or flush fails: the host already has the patch, so the
  snapshot **must** advance anyway.

Scratch restoration is unconditional across every exit, which means a guard rather than a sequence
of statements. Ratatui's current buffer is borrowed as scratch and must be left exactly as found;
its previous buffer stays behind the physical terminal, and the next ordinary draw resynchronises.

Remove the temporary eligible-frame log from `render_terminal_paint_only` in this commit.

Process damaged rows one at a time. Batching several regions into one walk is an optimisation for
later, if measurement asks for it.

### 3. Production-context oracle

The two discriminators ran through `render_headless`, whose context is deliberately simplified:
`images_enabled: false`, `contrast_policy: Off`, `blink_visible: true`, no drag or copy-feedback
state. The clipping property should not depend on those, but that is the assumption this step
exists to check.

At least one accepted case must use the same context construction as `draw_current_tree`, and
assert:

```
incrementally patched last_frame_snapshot  ==  ordinary production full paint
```

Priority: focused, non-default theme and terminal styles, selection, integrated scrollbar. Then
hover and `blink_visible`. Also cover the trailing-blank-rows case - a viewport taller than the
terminal content - which a row-only assertion misses and only a full-framebuffer comparison
catches.

Anything the planner already rejects (images, composite surfaces, scrolled back, `Full` damage,
decorated terminal, several terminals) needs a typed `DamageRejection` assertion and nothing more.

### 4. Benchmark

Rozi's harness, 30 Hz, one changed character, animations off. Baselines to beat:

| Viewport | Cells | Before |
| --- | ---: | ---: |
| 80x24 | 1,920 | 390 us/update |
| 200x60 | 12,000 | 1,723 us/update |
| 320x90 | 28,800 | 3,557 us/update |

Acceptance is at least 50% off at 200x60, but the shape matters more than the number: cost should
track row width plus a fixed tree traversal, not width times height. 320x90 should stop being about
nine times 80x24 for the same one-row mutation.

## After it clears

Merge, wire Rozi's pane-output path to `Update::terminal_paint()`, and reproduce the original
spinner case end to end. **Do not chase column-level `left`/`right` damage until that real-world
number is in.** The plan already carries the column span; row granularity turns 12,000 cells into
200 at 200x60, and if that does not make the real case negligible, column precision will not save
it either.
