# Contributing to tui-lipan

Thanks for considering a contribution! This document covers what you need to
know to land a PR.

## Quick checklist

Before opening a PR:

- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace --all-features` passes
- [ ] Macro-body formatting: `./scripts/format-rust-with-macros --check` passes
- [ ] User-visible changes are listed in `CHANGELOG.md` under `[Unreleased]`
- [ ] Docs in `docs/` are updated if the behavior or API surface changed
- [ ] If you added a new widget, all checklist steps in
      [`docs/widget-authoring.md`](docs/widget-authoring.md) are completed

## Toolchain

- **MSRV:** Rust 1.85 (`edition = "2024"`)
- Stable toolchain is expected for all CI jobs

## Local development

```bash
# Format
cargo fmt --all
./scripts/format-rust-with-macros src/ examples/ tests/ benches/ tui-lipan-macro/

# Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Test
cargo test --workspace --all-features

# Run an example
cargo run --example showcase
cargo run --example image --features image
cargo run --example markdown_hub --features markdown
```

For faster iteration on examples without paying full release-build costs:

```bash
cargo run --profile dev-fast --example scroll_view_opencode_repro \
    --features "markdown diff-view syntax-syntect"
```

## CHANGELOG policy

Every PR with a user-visible change **must** add an entry under `[Unreleased]`
in `CHANGELOG.md`. The format follows [Keep a Changelog](https://keepachangelog.com/):

```markdown
## [Unreleased]

### Added
- New `Foo` widget with `.bar()` builder.

### Changed
- `Frame::title` now accepts `impl Into<Cow<'static, str>>` (breaking).

### Fixed
- `ScrollView` no longer clips the last row when content height is odd.

### Removed
- Deprecated `LegacyButton` widget.
```

Use these section headings: **Added**, **Changed**, **Deprecated**, **Removed**,
**Fixed**, **Security**.

**Breaking changes** must say "(breaking)" at the end of the line so they are
trivial to grep at release time.

**Skip the changelog only for:** internal refactors with no API/behavior change,
docs-only changes, CI/tooling changes, test-only changes. When in doubt, add an
entry - it's cheaper than missing one.

## Adding a new widget

Start with a composite widget whenever the UI can be expressed using existing
primitives. New primitive widgets are framework-maintainer work and should meet
the acceptance criteria in [`docs/widget-authoring.md`](docs/widget-authoring.md):
they need custom measurement, node state, rendering, hit testing, or scrollbar
regions; fit the curated built-in set; and cannot be cleanly expressed as a
composite.

The full primitive checklist (which files to touch, in which order) lives in
[`docs/widget-authoring.md`](docs/widget-authoring.md). Skipping any step will
cause a non-obvious panic or render glitch - every match arm in the dispatch
chain is exhaustive.

After implementation:

1. Add a runnable example in `examples/<widget_name>.rs`.
2. Add a per-widget doc page or section in `docs/widgets/`.
3. Add a `CHANGELOG.md` entry under `### Added`.
4. If the widget is feature-gated, register the example in `Cargo.toml`
   under `[[example]]` with `required-features`.

## Releasing (maintainers)

1. Move `[Unreleased]` entries into a new `## [X.Y.Z] - YYYY-MM-DD` section.
2. Bump `version` in both `Cargo.toml` files (root and `tui-lipan-macro/`).
3. Update the dependency line `tui-lipan-macro = { ..., version = "X.Y.Z" }`.
4. Update the comparison links at the bottom of `CHANGELOG.md`.
5. Commit with message `release: vX.Y.Z`, tag `vX.Y.Z`, push.
6. Publish:
   ```bash
   cargo publish -p tui-lipan-macro
   # wait ~30s for the crates.io index to update
   cargo publish -p tui-lipan
   ```
7. Create a GitHub release referencing the changelog section.

## Filing issues

Bug reports - please include:
- `tui-lipan` version + enabled features
- Terminal emulator and OS
- Minimal reproducer (a small `#[example]` is ideal)

Feature requests - please include:
- The use case (what app you're building, what's blocked)
- A sketch of the API you'd want, even rough

## License and the DCO

tui-lipan is licensed under **MPL-2.0** (see [LICENSE](LICENSE)). Commercial
support and services are also available - see [COMMERCIAL.md](COMMERCIAL.md).

Contributions follow **inbound = outbound**: unless you state otherwise, any
contribution you intentionally submit for inclusion is licensed under the same
**MPL-2.0** as the project, with no additional terms. You retain the copyright
in your contributions - tui-lipan does **not** ask you to assign copyright or
sign a CLA.

Instead, we use the [Developer Certificate of Origin](https://developercertificate.org/)
(DCO): a lightweight, one-line attestation that you wrote the change (or
otherwise have the right to submit it) and agree to license it under MPL-2.0.
Sign off each commit by adding a `Signed-off-by` trailer:

```bash
git commit -s -m "fix(scroll_view): clip last row on odd content height"
```

This appends a line like:

```
Signed-off-by: Your Name <you@example.com>
```

The name and email must be real and match your Git identity. If you forget,
`git commit --amend -s` (or `git rebase --signoff` for a series) adds it.

> **Why DCO over a CLA?** A CLA would let the project relicense your code under
> proprietary terms later. We deliberately don't want that power: keeping
> everything under MPL-2.0 (inbound = outbound) is a promise that the framework
> stays open and cannot be quietly closed. The DCO gives us a clean provenance
> record without taking any extra rights from you.
