//! Kept design sketches.
//!
//! Every sketch in this directory stays checked in. A sketch is cheap to keep
//! (one view function plus a two-line `sketch()`), and once kept it doubles as a
//! visual regression check: re-run it after a refactor and compare the artifacts.
//!
//! Run every sketch:
//!
//! ```bash
//! cargo snap sketches
//! ```
//!
//! Run one by name:
//!
//! ```bash
//! cargo snap sketches -- login
//! ```
//!
//! Artifacts land in `target/ui-sketches/`, so nothing here needs a `.gitignore`
//! entry and `cargo clean` removes the output.
//!
//! # Adding a sketch
//!
//! 1. Add `examples/sketches/<name>.rs` with a `view()` and a `sketch()`, using
//!    `login.rs` as the template.
//! 2. Add `mod <name>;` and one row to [`SKETCHES`] below.
//!
//! No `Cargo.toml` change is needed: cargo discovers `examples/sketches/main.rs`
//! as a single example, so new sketches never touch build config.

mod login;

use tui_lipan::Result;

/// A sketch entry point: renders and writes its own artifacts.
type SketchFn = fn() -> Result<()>;

/// Every sketch that can be run by name.
const SKETCHES: &[(&str, SketchFn)] = &[("login", login::sketch)];

fn main() -> Result<()> {
    let requested: Vec<String> = std::env::args().skip(1).collect();

    let selected: Vec<&(&str, SketchFn)> = if requested.is_empty() {
        SKETCHES.iter().collect()
    } else {
        let mut selected = Vec::new();
        for name in &requested {
            match SKETCHES.iter().find(|(sketch, _)| sketch == name) {
                Some(entry) => selected.push(entry),
                None => {
                    eprintln!("unknown sketch `{name}`");
                    eprintln!("available: {}", available_names().join(", "));
                    std::process::exit(2);
                }
            }
        }
        selected
    };

    for (name, run) in selected {
        println!("-- {name}");
        run()?;
    }

    Ok(())
}

/// Names of every registered sketch, for the unknown-name hint.
fn available_names() -> Vec<&'static str> {
    SKETCHES.iter().map(|(name, _)| *name).collect()
}
