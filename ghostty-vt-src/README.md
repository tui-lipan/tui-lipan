# tui-lipan-ghostty-vt-src

The [libghostty-vt](https://lib.ghostty.org/vt/) 1.0.1 release source and its Zig package
dependencies, vendored for
[`tui-lipan-ghostty-vt-sys`](https://crates.io/crates/tui-lipan-ghostty-vt-sys). With it, that crate
builds the library with Zig 0.17 alone: no network, no `curl` and no `patch` at build time, so
offline, `cargo vendor` and sandboxed builds work.

The tree is produced by `scripts/package-ghostty-src.sh` in the tui-lipan repository:

1. Downloads the official `libghostty-vt-1.0.1.tar.gz` and `libghostty-vt-1.0.1-deps.tar.gz` and
   checks their pinned SHA-256 sums (and minisign signatures when `minisign` is installed).
2. Applies tui-lipan's patches (listed with their SHA-256 in `PATCHES`).
3. Removes tests, documentation and examples that the library build does not read.

`THIRD_PARTY.md` lists every vendored package with its version, licence and licence files. This
crate builds nothing itself; it only exposes the directory paths.
