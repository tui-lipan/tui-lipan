# Performance

Performance work on [opencode-tui](https://github.com/tui-lipan/opencode-tui) found
several hot paths caused by doing valid work at the wrong scope or frequency.
Large transcripts, streamed updates, syntax-highlighted documents, and diff
views became cheaper once the app stopped rebuilding them for unrelated input
and scroll events.

Use this order when tuning an application:

1. Measure a release build with representative data and a fixed viewport.
2. Reduce how often work runs.
3. Reduce the scope of each update.
4. Memoize or cache only the expensive work that remains.
5. Bound retained data and background work.

## Choose the smallest `Update`

An update's refresh level determines how much of the UI pipeline runs:

| Return | Use when |
|--------|----------|
| `Update::none()` | Only non-visual metadata changed, or a widget already updated its runtime-owned visual state |
| `Update::paint()` | The realized tree only needs repainting; no `view()` output or layout changed |
| `Update::layout()` | The emitting component's subtree changed |
| `Update::layout_with_command(cmd)` | The local subtree changed and background work must start |
| `Update::full()` | Root composition or another component scope depends on the change |
| `Update::with_command(cmd)` | A root-wide change also starts background work |
| `Update::command_only(cmd)` | Work must start, but no immediate visual state changed |

High-frequency callbacks deserve special attention. Scroll telemetry, drag
positions, cursor synchronization, hidden-screen cache updates, and stale-result
bookkeeping often need `Update::none()`, not `Update::full()`.

`Update::paint()` is not a general shortcut for `layout()`: use it only when
rerunning `view()` would produce the same element tree. See
[Components](components.md#the-update-return-type) for the complete update model.

## Let widgets own high-frequency state

Avoid mirroring widget-owned state back into controlled props on every event. In
particular:

- Leave `ScrollView` uncontrolled during wheel and scrollbar interaction.
- Treat controlled offsets as one-shot programmatic requests, then release
  control after the widget reports the target position.
- Return `Update::none()` from viewport callbacks that only record offset or
  visibility metadata.
- Use `ScrollRequest`, keyed targets, `scroll_to_bottom()`, and a stable
  `scroll_state_key` instead of app-side page calculations or sentinel rows.
- Request layout only when a scroll event changes app-rendered chrome, such as a
  sticky header or a newly consumed anchor.

This prevents a feedback loop where scrolling changes props, reruns `view()`,
remeasures the document, and changes the controlled offset again. See the
[ScrollView scrolling model](widgets/layout.md#scrolling-model).

## Isolate expensive subtrees

Move a costly transcript, diff pane, or tool output into a child `Component` and
give it a semantic `memo_key()`. The key should contain every input that changes
the rendered subtree, but nothing else.

Useful patterns from opencode-tui:

- Reduce raw dimensions to the mode that affects output. For example, key a diff
  pane by split versus unified mode rather than every intermediate terminal
  width when layout already handles wrapping.
- Exclude callbacks from memo identity only when they are behaviorally identical
  and capture no changing data, including IDs or routing context.
- Cache derived values against a narrow content revision, not a generic render
  count.
- Test both sides of the contract: unrelated changes must retain the subtree,
  while every structural or content change must invalidate it.

For a smaller expensive region inside `view()`, use
`Memo::new(deps_hash).build(...)`. The dependency hash must include every
captured value that affects output. Use `Memo::with_call_site(...)` when a shared
helper creates memos, so separate call sites cannot collide.

`Component::memo_key()` retains a component subtree, while `.key(...)` preserves
reconciliation identity. A stable key is important for dynamic rows, focus, and
reorders, but does not enable memoization by itself. See
[Retained subtree reuse](components.md#retained-subtree-reuse).

## View timings and tracing

With `devtools` metrics visible, exclusive (self) `view()` time is sampled per
component and summed across stability passes. The overlay lists the slowest
views. With `profiling-tracing`, spans `component.view` / `component.refresh`
carry `component` and `scope` fields; `app.render_full` includes `root`.

## Input pressure

When many recent Full frames are both driven by input attributions and exceed
~16ms, the panel's `Input` row switches from `ok` to
`Input  N/60 full frames over budget`. Prefer Layout/Paint updates from
handlers, memoized subtrees, and `_arc` props — the signal is informational
only (no log spam).

## Memo miss reasons

With the `devtools` feature enabled, the stats panel's `Miss` row shows the top
miss reasons over the recent frame window. Reason bookkeeping (and the extra dependency
probe behind it) only runs while the panel is visible with `metrics: true`, so
shipping with `devtools` compiled in adds nothing beyond the plain hit/miss
counters until the panel is opened. Component retains report `no-cache`,
`key`, `dirty`, `dep:*` (theme/focus/hover/scroll/viewport/context/…), or
`child-refresh`. In-view `Memo` nodes report `view-cache`, `view-deps`, or
`view-structure`, and count toward the hit rate.

Only components with a `memo_key()` (and in-view `Memo` nodes) participate in
the hit/miss stats; plain components are not counted as misses. Counters move
when components actually re-expand (full renders and scoped layout refreshes),
so an idle app or a purely paint/layout-driven burst legitimately shows
`no data`.

## Keep props cheap and stable

Large immutable render inputs should be shared rather than cloned or deeply
compared on every streamed update:

- Collection widgets expose paired `x(impl IntoIterator)` + `x_arc(Arc<[T]>)`
  setters and store collections as `Arc<[T]>` (for example `List::items_arc`,
  `Table::rows_arc`, `Tabs::tabs_arc`, `Chart::series_arc` /
  `ChartSeries::data_arc`, `Sparkline::data_arc`, `MultiSelect::items_arc`,
  `SearchPalette::items_arc` / `entries_arc`, `LogView::entries_arc`). Prefer
  holding the `Arc` in component state and passing it through `_arc` when the
  collection is unchanged between frames.
- Tiny label collections (`Radio`, `Breadcrumb`, `ComboBox`), recursive trees
  (`Tree` children), filesystem-sourced `FileTree`, and text-content widgets
  (`DiffView`, `DocumentView`) intentionally skip the convention — the clone
  win is negligible or a shared top slice would still deep-clone on child edit.
- Put an `Arc::ptr_eq` fast path first in hand-written prop equality, followed by
  an allocation-free structural comparison when different allocations can still
  represent equal content.
- Do not rely on pointer-keyed layout-hash memoization of `Arc` collections:
  naive `as_ptr` hashing risks spurious relayouts when equal content lives in a
  new allocation. Content hashing remains the layout-hash contract; `_arc`
  setters are caller-side clone avoidance only. Pointer-keyed layout memoization
  remains future work if a safe epoch or generation scheme is designed.
- Use an explicit content epoch when identity fields do not capture in-place
  content changes.
- Make hand-written `PartialEq` implementations exhaustive so adding a prop
  forces its render semantics to be considered.
- Build invariant catalogs, color maps, parsed assets, and formatter inputs once,
  outside `view()`.

Do this for data that is large, shared, or expensive to compare. Small values do
not need `Arc` merely for consistency.

## Bound work before layout

The fastest row to measure is the row the app never builds:

- Cap long histories and search results to the amount the UI can use.
- Collapse large tool output by default; only build the full document when the
  user expands it.
- Give immediate `ScrollView` children stable semantic keys. Set
  `estimated_child_height` when the default estimate differs materially from
  typical rows so virtual measurement can converge quickly.
- Cache repeated parsing or preprocessing with an explicit size bound and
  invalidation rule.
- Prefer framework `DocumentView`, `DiffView`, and syntax formatters over local
  implementations so formatting, measurement, and cache identity stay under one
  contract.

Do not add app-side list virtualization until measurement shows that
`ScrollView`'s virtual measurement is insufficient.

## Coalesce background and streamed work

Never block `view()` or normal `update()` with filesystem, network, parsing, or
expensive search work. Run it through `ctx.link().command(...)` or
`Command::spawn`, and use the narrow update that matches the immediate UI
change. `Command::new` runs synchronously on the UI thread.

For superseding work such as filter-as-you-type, use a keyed command with
`TaskPolicy::LatestOnly`. Cancellation is cooperative, so check
`is_cancelled()` and use `send_if_not_cancelled(...)` before publishing results.

For streamed views:

- Apply events incrementally instead of refetching and rebuilding the whole
  document after each token.
- Filter events by semantic relevance and debounce bursty refreshes, while
  preserving a final trailing refresh.
- Keep one authoritative reducer and monotonic sequence for live and cached
  views. Reject stale snapshots and mark invalidation explicitly.
- Prune side maps and cached derived state when their owning rows are evicted.

These are app architecture decisions, but they determine how often tui-lipan is
asked to reconcile and lay out a large tree.

Prefer built-in spinners, transitions, and effects over an app-owned 16 ms
command loop. Remove completed effects and lower custom effect cadence where
possible so an idle app returns to event-driven rendering.

## Diagnose before optimizing

Enable the built-in panel while investigating update scope and subtree reuse:

```toml
tui-lipan = { version = "*", features = ["devtools"] }
```

```rust
App::new().devtools_config(DevToolsConfig {
    logs: false,
    metrics: true,
    show_framework_logs: false,
})
```

The stats panel renders a fixed set of rows so nothing appears or disappears
between frames, and every value aggregates over the last 60 recorded frames
rather than the latest one (per-frame data at full frame rate is unreadable).
Rows top to bottom:

- `FPS / Nodes / Overlays`: headline counters (latest frame).
- `Frame` / `Recon` / `Draw`: average and worst frame time over the window.
- `Chart`: frame-time bar chart, one column per recorded frame. The scale
  floor is one 60fps frame budget (16.7ms), stretched by the worst frame in
  view; bar heights are square-root compressed so typical sub-millisecond
  frames stay visible next to a spike. Spike tops render in the accent color.
- `Updates`: how many window frames were full, layout-only, or paint-only.
- `Why`: top update sources (components and input paths) merged across the
  window, e.g. `input:scroll x120 · Sidebar x10`, or `idle`. Animation ticks,
  resize, and other framework-internal dirty marks are not attributed.
- `Memo` / `Miss`: window hit rate and top miss reasons.
- `Slow`: worst single-frame `view()` time per component.
- `Focus`: focused tag and key, focus policy, ring size (`r4` = 4 tab stops).
- `Input`: `ok`, or the input-pressure warning when input-driven full frames
  repeatedly blow the frame budget.

The overlay and sampling slightly perturb the workload, so use tracing or a
benchmark for final comparisons.

For runtime spans and timing events, enable instrumentation and install a
subscriber in the app binary:

```toml
[dependencies]
tui-lipan = { version = "*", features = ["profiling-tracing"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

```rust
tracing_subscriber::fmt()
    .with_env_filter("tui_lipan=trace,tui_lipan::perf=trace")
    .init();
```

The feature emits spans and events for full, layout-only, and paint-only frames,
drawing, reconciliation, and `DocumentView` formatting/cache work. A CPU profiler
should use an optimized build with debug symbols so inlined hot frames remain
visible.

Measure startup separately from steady-state rendering. Terminal capability
queries and readiness probes must have short bounds; otherwise a silent PTY or
test harness can look like a slow first frame even when no app view is running.

## Benchmarking

Use `TestBackend` with a fixed viewport to benchmark application messages and
view/reconcile/layout work without terminal variance. Construct and warm the
backend outside the timed loop, then benchmark representative mutations such as
a streamed update, cache miss, width change, reorder, or far scroll jump. Call
`capture_frame()` when the benchmark should include backend painting.

Use a separate PTY/process harness for startup, ready-frame latency, aggregate
CPU, and resident memory. Keep data, features, viewport, power state, terminal,
and build profile constant between comparisons.

tui-lipan's Criterion targets are:

```bash
cargo bench --bench document_view_markdown --features markdown
cargo bench --bench text_area_reconcile
cargo bench --bench document_view_wrap --features markdown
cargo bench --bench scroll_view_rich_children --features markdown
cargo bench --bench scroll_reorder_reconcile
```

Save and compare a baseline with Criterion's standard flags:

```bash
cargo bench --bench document_view_markdown --features markdown -- --save-baseline stable1
cargo bench --bench document_view_markdown --features markdown -- --baseline stable1
```

Read Criterion results by relative change and confidence interval, not one
absolute run. Treat very small changes as noise unless they reproduce; warm up,
alternate comparison order, and rerun before drawing conclusions.

For `DocumentView`, optimize cache misses first, width-driven reflow second, and
warm unchanged renders last. The warm path is already expected to be cheap.

## Checklist

- Does idle avoid unnecessary app-driven frames when no cursor, spinner,
  transition, or effect is active?
- Do high-frequency callbacks use the smallest correct `Update`?
- Does user scrolling remain widget-owned?
- Are expensive children keyed and memoized by semantic content only?
- Are large props shared and compared without temporary allocations?
- Are histories, outputs, caches, and pending work bounded?
- Is blocking work command-driven, cancellable, and coalesced?
- Was the change measured in a release build with representative data?
