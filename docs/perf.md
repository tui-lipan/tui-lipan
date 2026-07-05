# Performance and Benchmarking

This guide documents the supported way to measure tui-lipan performance.

Use two layers:

1. **Criterion benches** for repeatable regressions and optimization tracking.
2. **Runtime tracing** for live frame diagnostics inside real apps.

## 1) Criterion Benchmarks

Current benchmark targets:

- `document_view_markdown_formatter/`*
- `document_view_markdown_reconcile/*`
- `text_area_reconcile/*`
- `document_view_wrap/*`
- `scroll_view_rich_children/*`

Run:

```bash
cargo bench --bench document_view_markdown --features markdown
```

```bash
cargo bench --bench text_area_reconcile
```

```bash
cargo bench --bench document_view_wrap --features markdown
```

```bash
cargo bench --bench scroll_view_rich_children --features markdown
```

### Save a baseline

```bash
cargo bench --bench document_view_markdown --features markdown -- --save-baseline stable1
```

PreparedText/TextArea baseline workflow:

```bash
cargo bench --bench text_area_reconcile -- --save-baseline before-prepared-text
```

### Compare against a baseline

```bash
cargo bench --bench document_view_markdown --features markdown -- --baseline stable1
```

```bash
cargo bench --bench text_area_reconcile -- --baseline before-prepared-text
```

## 2) Runtime Profiling with tracing

Enable framework instrumentation:

```toml
tui-lipan = { version = "*", features = ["profiling-tracing"] }
```

Then install your own subscriber in the app binary. Minimal example:

```rust
tracing_subscriber::fmt()
    .with_env_filter("tui_lipan=trace,tui_lipan::perf=trace")
    .init();
```

You can also use:

- `tracing-tracy` for timeline/profiler UI.
- OpenTelemetry exporters for centralized traces.

## How to Read Criterion Output

Example shape:

```text
time:   [a b c]
change: [-X% ...] (p = 0.00 < 0.05)
```

- `time [a b c]` = confidence interval (lower, center, upper).
- `change` = comparison vs previous/baseline.
- `p < 0.05` = statistically significant.
- `Performance has improved` = significant and above Criterion noise threshold.
- `Change within noise threshold` = small shift, not actionable.

`Gnuplot not found, using plotters backend` is informational only.

## Practical Interpretation Rules

- Prefer **relative** changes over absolute numbers.
- Track at least these hot paths:
  - formatter: `small`, `medium`, `large`
  - reconcile: `warm_cache_render`, `cache_miss_value_toggle`, `reflow_on_width_change`
- Treat tiny deltas (<~1%) as likely noise unless repeatedly reproduced.
- Outliers are normal on dev machines; focus on medians and confidence intervals.

## Run Hygiene (for stable numbers)

- Close heavy background apps.
- Use consistent power mode (avoid CPU governor changes).
- Keep terminal/session setup constant.
- Re-run at least 2-3 times before concluding regressions.

## Optimization Priority Order (DocumentView)

When chasing markdown speed, optimize in this order:

1. `cache_miss_value_toggle` (largest cost).
2. `reflow_on_width_change` (layout pressure).
3. `warm_cache_render` (already cheap, optimize last).
