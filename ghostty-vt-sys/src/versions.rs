// Shared by `build.rs` (through `#[path]`) and the crate, so the build error, the crate docs and
// the build cannot disagree about the toolchain. Keep this file free of `use` items.

/// The libghostty-vt release this crate binds and builds.
pub const LIBGHOSTTY_VT_VERSION: &str = "1.0.1";

/// The Zig release series that builds [`LIBGHOSTTY_VT_VERSION`] (`minimum_zig_version` in its
/// `build.zig.zon`). Any `0.17.x` works; other minor versions fail with unrelated compile errors,
/// which is why the build script checks this before running Zig.
pub const ZIG_VERSION: &str = "0.17";
