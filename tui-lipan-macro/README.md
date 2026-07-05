# tui-lipan-macro

Procedural macros for [**tui-lipan**](https://crates.io/crates/tui-lipan), the
opinionated, component-based TUI framework for Rust.

This crate provides the `ui!`, `rsx!`, and `mockup!` macros. You normally do not
depend on it directly - it is re-exported through `tui-lipan`:

```toml
[dependencies]
tui-lipan = "0.1"
```

```rust
use tui_lipan::prelude::*;
```

See the [tui-lipan documentation](https://docs.rs/tui-lipan) and
[repository](https://github.com/tui-lipan/tui-lipan) for usage, examples, and the
full feature set.

## License

Licensed under the **Mozilla Public License 2.0** (MPL-2.0), the same as
tui-lipan. See the [LICENSE](https://github.com/tui-lipan/tui-lipan/blob/main/LICENSE).
