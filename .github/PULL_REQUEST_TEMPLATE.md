<!--
PR titles land in main's history: PRs are squash-merged, so the title becomes
the squashed commit subject. Make the title a valid Conventional Commit:

  <type>(<optional scope>): <imperative summary>   (<= 72 chars, no trailing period)

  types: feat, fix, docs, refactor, test, style, perf, chore, ci, release
  examples:
    fix(scroll_view): clip last row on odd content height
    feat(modal): add max_height + reserve_max_height

Call out breaking changes and concrete migration steps in the summary.
-->

## Summary

<!-- What does this change and why? One or two sentences is fine. -->

## Checklist

Mirrors `CONTRIBUTING.md` (single source of truth). Run before requesting review:

- [ ] `cargo fmt --all` passes
- [ ] `./scripts/format-rust-with-macros --check` passes (macro-body formatting)
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` passes
- [ ] `cargo test --workspace --all-features` passes
- [ ] User-visible and breaking changes are explained, with migration steps where needed
- [ ] Docs in `docs/` updated if behavior or public API changed
- [ ] New widget? All steps in `docs/widget-authoring.md` are complete
- [ ] Every commit is signed off (`git commit -s`, DCO)

## Notes for reviewers

<!-- Screenshots/GIFs for UI changes, tricky areas, follow-ups. Optional. -->
