# tui-lipan Documentation

Welcome to the documentation for **tui-lipan** - an opinionated, component-based
TUI framework for Rust.

This site mirrors the [`docs/` folder in the GitHub repository](https://github.com/tui-lipan/tui-lipan/tree/main/docs)
and is rebuilt on every push to `main`. If a page looks out of date, click the
**edit** icon in the top-right corner to open it on GitHub.

## Where to start

- **New here?** Read the [Quick Start](./quick-start.md) - five minutes from
  zero to running app.
- **Learning by example?** The [tutorial](./tutorial.md) walks through a complete
  application end-to-end.
- **Building a large app?** See [Large app shells](large-app-shells.md) -
  structure and diagnostics for multi-pane, root-routed apps.
- **App feels slow?** See [Performance](./perf.md) for update granularity,
  scrolling, memoization, tracing, and repeatable benchmarks.
- **Reference?** Jump straight into [Components](./components.md),
  [UI Macros](./macros.md), or the [widget reference](./widgets/index.md).

## What's outside this site

| Where | What you'll find there |
|-------|------------------------|
| [tui-lipan.dev](https://tui-lipan.dev) | Landing page, install snippet, hero examples |
| [docs.rs/tui-lipan](https://docs.rs/tui-lipan) | Auto-generated rustdoc API reference |
| [github.com/tui-lipan/tui-lipan](https://github.com/tui-lipan/tui-lipan) | Source code, issues, releases |
| [github.com/tui-lipan/tui-lipan/tree/main/examples](https://github.com/tui-lipan/tui-lipan/tree/main/examples) | Runnable demos |

## Status

tui-lipan is **approaching stability**. The widget set, runtime model, and
public API surface are mature. While the crate is on `0.x.y`:

- A **minor** bump (`0.1` → `0.2`) signals breaking changes.
- A **patch** bump (`0.1.0` → `0.1.1`) is backward-compatible only.

All changes are tracked in [`CHANGELOG.md`](https://github.com/tui-lipan/tui-lipan/blob/main/CHANGELOG.md).

## License

Dual-licensed under **MIT OR Apache-2.0**
([MIT](https://github.com/tui-lipan/tui-lipan/blob/main/LICENSE-MIT) or
[Apache-2.0](https://github.com/tui-lipan/tui-lipan/blob/main/LICENSE-APACHE)) at
your option. You can build closed-source, proprietary, and commercial apps on top
of tui-lipan freely. Commercial
[support and services](https://github.com/tui-lipan/tui-lipan/blob/main/COMMERCIAL.md)
are also available.
