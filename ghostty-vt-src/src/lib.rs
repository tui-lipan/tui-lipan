//! The libghostty-vt 1.0.1 release source and its Zig package dependencies, vendored for
//! [`tui-lipan-ghostty-vt-sys`](https://crates.io/crates/tui-lipan-ghostty-vt-sys), so that crate
//! builds the library with Zig alone: no network, no `curl`, no `patch`.
//!
//! The tree is the official release tarball (`libghostty-vt-1.0.1.tar.gz`) with tui-lipan's
//! patches applied, and the official dependency tarball (`-deps.tar.gz`), both checked against
//! pinned SHA-256 sums, then trimmed of tests, documentation and examples the library build does
//! not read. `PATCHES` names each applied patch and its SHA-256; `THIRD_PARTY.md` lists every
//! vendored package with its licence. `scripts/package-ghostty-src.sh` in the tui-lipan repository
//! produces this crate.
//!
//! The directories are inputs for `zig build`; this crate builds nothing itself.

use std::path::Path;

/// The libghostty-vt release vendored here.
pub const LIBGHOSTTY_VT_VERSION: &str = "1.0.1";

/// The crate root, which holds `PATCHES` and `THIRD_PARTY.md`.
#[must_use]
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The patched libghostty-vt source tree, the directory to run `zig build` in.
#[must_use]
pub fn source_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/libghostty-vt"))
}

/// The unpacked Zig packages, the directory to pass to `zig build --system`.
#[must_use]
pub fn deps_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/libghostty-vt-deps"))
}

/// The manifest of applied patches: one `<patch path> <sha256>` line each.
#[must_use]
pub fn patches_file() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/PATCHES"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_tree_is_the_named_release() {
        let version = std::fs::read_to_string(source_dir().join("LIB_VERSION"))
            .expect("the vendored source holds LIB_VERSION");
        assert_eq!(version.trim(), LIBGHOSTTY_VT_VERSION);
        assert!(deps_dir().is_dir());
        assert!(patches_file().is_file());
    }
}
