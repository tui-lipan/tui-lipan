//! tui-lipan's changes to libghostty-vt, applied to a `GHOSTTY_VT_SOURCE_DIR` tree; the vendored
//! source already carries them. One file, described in its preamble.

use super::{BuildResult, io_err, sha256_file};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub const PATCH: &str = "patches/libghostty-vt.patch";

/// Written next to every library this script installs, naming the patches built into it, so a
/// later build can take that library through `GHOSTTY_VT_LIB_DIR` without rebuilding.
pub const MARKER: &str = "ghostty-vt-sys.patches";

/// The contents of [`MARKER`] for the patches of this crate version.
pub fn marker() -> BuildResult<String> {
    let digest = sha256_file(&patch_path())?;
    Ok(format!("{PATCH} {digest}\n"))
}

fn patch_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(PATCH)
}

/// Copies `source` into the build directory, applying [`PATCH`] when `apply` is set, and returns
/// the copy. Zig then never writes into the vendored crate (read-only in Nix or a registry) or a
/// caller-owned tree. The build key includes the patch digest, so the archive cannot be stale.
pub fn prepare(source: &Path, build_dir: &Path, apply_patch: bool) -> BuildResult<PathBuf> {
    let patched = build_dir.join("source");
    let _ = fs::remove_dir_all(&patched);
    copy_tree(source, &patched)?;
    if !apply_patch {
        return Ok(patched);
    }
    let patch = patch_path();
    let status = apply(&patch, &patched, build_dir)?;
    if !status.success() {
        return Err(format!(
            "applying {} to {} failed ({status}); the fix requires libghostty-vt 1.0.1 sources",
            patch.display(),
            source.display(),
        ));
    }
    Ok(patched)
}

/// Applies `patch` with `patch`, or with `git apply` where there is no `patch` (Windows without
/// MSYS tools on `PATH`). Both refuse any hunk that does not match exactly.
fn apply(patch: &Path, dir: &Path, build_dir: &Path) -> BuildResult<ExitStatus> {
    let with_patch = Command::new("patch")
        .current_dir(dir)
        .args(["--batch", "--forward", "--fuzz=0", "-p1", "-i"])
        .arg(patch)
        .status();
    match with_patch {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        other => return other.map_err(|e| format!("could not run patch ({e})")),
    }
    Command::new("git")
        .current_dir(dir)
        .args(["apply", "-p1"])
        .arg(patch)
        // The build directory usually sits inside the consumer's checkout, where `git apply`
        // would resolve paths against that repository instead of the source tree.
        .env("GIT_CEILING_DIRECTORIES", build_dir)
        .status()
        .map_err(|e| {
            format!("could not run `patch` or `git` ({e}); install either to build libghostty-vt")
        })
}

fn copy_tree(source: &Path, dest: &Path) -> BuildResult<()> {
    fs::create_dir_all(dest).map_err(io_err("create", dest))?;
    for entry in fs::read_dir(source).map_err(io_err("read", source))? {
        let entry = entry.map_err(io_err("read entry", source))?;
        let name = entry.file_name();
        if matches!(name.to_str(), Some(".git" | ".zig-cache" | "zig-out")) {
            continue;
        }
        let path = entry.path();
        let to = dest.join(name);
        if path.is_dir() {
            copy_tree(&path, &to)?;
        } else {
            fs::copy(&path, &to).map_err(io_err("copy", &path))?;
        }
    }
    Ok(())
}
