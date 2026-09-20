# Contributing to tui-lipan

Thanks for considering a contribution! This document covers what you need to
know to land a PR.

## Quick checklist

Before opening a PR:

- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` passes
      (catches broken intra-doc links, which the other lints do not)
- [ ] `cargo test --workspace --all-features` passes
- [ ] Macro-body formatting: `./scripts/format-rust-with-macros --check` passes
- [ ] The PR summary calls out user-visible and breaking changes with migration steps
- [ ] Docs in `docs/` are updated if the behavior or API surface changed
- [ ] If you added a new widget, all checklist steps in
      [`docs/widget-authoring.md`](docs/widget-authoring.md) are completed
- [ ] If `tests/visual_baseline.rs` failed, the rendering change was intended and
      the re-recorded images in `tests/ui-baselines/` are committed
      (see [Visual baselines](#visual-baselines))

Opening the PR fills in [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md)
automatically - keep its checklist.

## Visual baselines

`tests/visual_baseline.rs` renders core widget chrome - frame borders and
headers, focus chrome, input placeholders and masking, list selection - and
compares it against committed reference images in `tests/ui-baselines/`. It
exists so a refactor that quietly moves a border or drops a focus highlight fails
in CI instead of shipping.

A failure is not automatically a bug. Read the `*.diff.png` path named in the
failure message: unchanged pixels are dimmed, changed pixels are magenta, so what
moved is obvious. Then decide.

- **The change was intended** (you restyled a widget): re-record and commit the
  updated images in the same PR.

  ```bash
  TUI_LIPAN_UPDATE_BASELINES=1 cargo test --all-features --test visual_baseline
  ```

- **The change was not intended**: you found a rendering regression. Fix it
  rather than updating the baseline.

Diff images are gitignored; only the baselines themselves are committed.
Comparison always uses the crate's built-in bitmap font rather than a system
font, so results are identical on CI and on every contributor's machine - a
baseline never fails because of which fonts you have installed.

## Pull request titles

PRs are **squash-merged**, so the PR title becomes the commit subject on `main`.
Give it a Conventional Commit title - same format as commits (see below):
`<type>(<optional scope>): <imperative summary>`, `<= 72` chars, no trailing
period. For example: `fix(scroll_view): clip last row on odd content height`.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/): a `<type>`
(`feat`, `fix`, `docs`, `refactor`, `test`, `style`, `perf`, `chore`, `ci`,
`release`) with an optional scope, an imperative summary, `<= 72` chars, and no
trailing period. Explain breaking changes and their migration steps in the PR
summary and relevant documentation.

## Toolchain

- **MSRV:** Rust 1.90 (`edition = "2024"`)
- Stable toolchain is expected for all CI jobs

The crate's own code and its default features build on 1.88, but the `image`
feature (and everything that enables it — `terminal-images`,
`image-full-formats`, `clipboard-images` + `ratatui-image`) pulls
`ratatui-image → icy_sixel → quantette`, and `quantette 0.5.1` declares
`rust-version = "1.90"`. `icy_sixel 0.5.0` pins `quantette = "0.5.1"`, so there
is no older resolution that avoids it. `rust-version` is a single
package-level value with no per-feature form, so the manifest declares the
ceiling the full feature set actually needs.

## Local development

```bash
# Format
cargo fmt --all
./scripts/format-rust-with-macros src/ examples/ tests/ benches/ tui-lipan-macro/

# Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

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

## Release notes

[GitHub Releases](https://github.com/tui-lipan/tui-lipan/releases) is the
canonical changelog. The repository intentionally has no `CHANGELOG.md` and
does not use per-PR note fragments.

The `/changelog` command is the source of truth for release-note structure.
Rosie writes the prose from each PR Summary. Keep titles and summaries useful:

- Keep PR titles specific and use a Conventional Commit type that matches the
  primary user-visible effect.
- Put the user-visible effect in the Summary: what broke or changed, and what
  happens now. Name APIs, defaults, feature flags, MSRV changes, and migration
  steps there so the notes can repeat them.
- Give a concrete replacement or migration step for every breaking change.
- Update app-author documentation and examples in the same PR.

Internal refactors, tests, formatting, CI, and documentation-only commits are
excluded from release notes.

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
3. If the widget is feature-gated, register the example in `Cargo.toml`
   under `[[example]]` with `required-features`.

## Releasing (maintainers)

1. Keep `GOOGLE_GENERATIVE_AI_API_KEY` configured in the protected `release`
   environment. Rosie uses `google/gemini-3.5-flash-lite`; OpenCode receives no
   GitHub or crates.io publication token.
2. Bump `version` in both `Cargo.toml` files and update the root
   `tui-lipan-macro` dependency to the same version.
3. Run `python3 -m unittest scripts.test_release_notes` with the normal
   formatting, lint, documentation, and test checks.
4. Commit with message `release: vX.Y.Z` and a DCO sign-off, create the
   `vX.Y.Z` tag, and push the commit and tag.
5. `.github/workflows/release.yml` resolves the previous published Release and
   the new tag to exact commits, proves ancestry, and gives Rosie every
   candidate commit subject, summary, and changed path. Rosie writes `Added`,
   `Changed`, `Fixed`, `Compatibility`, and `Security` sections from those
   summaries.
6. The workflow validates and freezes the generated Markdown as an artifact.
   Note generation is a hard gate: a missing key, model failure, permission
   failure, or invalid output stops publication.
7. After verification and note generation succeed, Trusted Publishing
   publishes `tui-lipan-macro` and then `tui-lipan`. The workflow creates the
   GitHub Release from the same frozen notes.

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
