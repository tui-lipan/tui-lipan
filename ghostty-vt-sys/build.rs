//! Builds and links libghostty-vt.
//!
//! By default this builds the static library with Zig from the patched release source that the
//! `tui-lipan-ghostty-vt-src` build-dependency vendors, into a cache shared by every profile and
//! feature set of the workspace, so a rebuild does not recompile. No network access is needed.
//!
//! Environment (all optional):
//!
//! | Variable | Meaning |
//! | --- | --- |
//! | `GHOSTTY_VT_LIB_DIR` | Link a prebuilt library from this directory instead of building. |
//! | `GHOSTTY_VT_LINK` | `static` or `dylib` for `GHOSTTY_VT_LIB_DIR`/pkg-config. Default `static`, `dylib` on MSVC. |
//! | `GHOSTTY_VT_SYSTEM` | `1` to find an installed library with pkg-config. |
//! | `GHOSTTY_VT_ALLOW_UNPATCHED` | `1` to link a system or prebuilt library that lacks [`patch::PATCH`]. |
//! | `GHOSTTY_VT_SOURCE_DIR` | An unpacked, unpatched `libghostty-vt-1.0.1` source tree to build instead of the vendored one. [`patch::PATCH`] is applied to a private copy, which needs `patch` or `git`. |
//! | `GHOSTTY_VT_DEPS_DIR` | Its unpacked `-deps` directory (Zig packages). Required with `GHOSTTY_VT_SOURCE_DIR`. |
//! | `GHOSTTY_VT_CACHE_DIR` | Build cache. Default `<target>/ghostty-vt`. |
//! | `GHOSTTY_VT_OPTIMIZE` | Zig optimize mode. Default `ReleaseFast`, also for debug profiles. |
//! | `GHOSTTY_VT_CPU` | Zig `-Dcpu`. Default `baseline`, so binaries run on any CPU of the target. |
//! | `GHOSTTY_VT_ZIG_TARGET` | Zig `-Dtarget`, used verbatim. Overrides the two below. |
//! | `GHOSTTY_VT_GLIBC` | Oldest glibc to support on `*-linux-gnu`, e.g. `2.28`. Default: the host's. |
//! | `MACOSX_DEPLOYMENT_TARGET` | Oldest macOS to support, as for rustc. Default: libghostty-vt's 13.0. |
//! | `ZIG` | The Zig executable ([`versions::ZIG_VERSION`]`.x`). Default `zig` on `PATH`. |
//!
//! A native build targets the build machine, including its glibc: right for `cargo install`, wrong
//! for a binary shipped to older systems. Distribution builds set `GHOSTTY_VT_GLIBC` (or the full
//! `GHOSTTY_VT_ZIG_TARGET`).
//!
//! The default build needs only Zig [`versions::ZIG_VERSION`]`.x`: the vendored source already
//! carries [`patch::PATCH`] (its `PATCHES` manifest must match this crate's patch), so offline,
//! `cargo vendor` and sandboxed builds work.
//!
//! Windows MSVC builds the static library with Zig's `windows-msvc` target, which needs the same
//! Visual Studio C++ tools and Windows SDK that rustc's MSVC target links with.
//!
//! Cache entries are keyed by the compiled recipe (`build.rs`, all `build/*.rs`, versions and
//! this crate's manifest), the pinned source package manifest and PATCHES, and Zig settings.
//! Old keys are never reused. Prefixes are kept because another Cargo process or cached build
//! script can still link them. To reclaim old entries, stop builds using this cache and remove
//! its `build/` directory; Cargo watches linked archives and rebuilds missing entries. CI uses
//! exact cache keys only, so obsolete recipes are not folded into subsequent cache uploads.
//!
//! On docs.rs (`DOCS_RS`) nothing is built or linked: the bindings are committed and rustdoc
//! never links.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

#[path = "build/cache.rs"]
mod cache;
#[path = "build/patch.rs"]
mod patch;
#[path = "src/versions.rs"]
#[allow(dead_code)]
mod versions;

/// The libghostty-vt release this crate binds. The bindings in `src/bindings.rs` were generated
/// from the headers of exactly this release.
const VERSION: &str = versions::LIBGHOSTTY_VT_VERSION;

const ENV_VARS: &[&str] = &[
    "GHOSTTY_VT_LIB_DIR",
    "GHOSTTY_VT_LINK",
    "GHOSTTY_VT_SYSTEM",
    "GHOSTTY_VT_SOURCE_DIR",
    "GHOSTTY_VT_DEPS_DIR",
    "GHOSTTY_VT_CACHE_DIR",
    "GHOSTTY_VT_OPTIMIZE",
    "GHOSTTY_VT_CPU",
    "GHOSTTY_VT_ZIG_TARGET",
    "GHOSTTY_VT_GLIBC",
    "GHOSTTY_VT_ALLOW_UNPATCHED",
    "MACOSX_DEPLOYMENT_TARGET",
    "DOCS_RS",
    "ZIG",
];

type BuildResult<T> = Result<T, String>;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", patch::PATCH);
    println!("cargo:rerun-if-changed=build");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src/versions.rs");
    for var in ENV_VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    // docs.rs has neither network nor Zig. The bindings are committed and rustdoc never links,
    // so there is nothing to build.
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    if let Err(message) = run() {
        // A panic prints the message in full, where a plain exit would hide it behind the
        // "failed to run custom build command" summary.
        panic!("\n\nghostty-vt-sys: {message}\n\n");
    }
}

fn run() -> BuildResult<()> {
    let target = env::var("TARGET").map_err(|e| e.to_string())?;
    let msvc = target.ends_with("-msvc");
    let link = link_kind(msvc)?;

    if env_flag("GHOSTTY_VT_SYSTEM") {
        require_patched(None, "the pkg-config libghostty-vt (GHOSTTY_VT_SYSTEM)")?;
        return link_pkg_config(link);
    }
    if let Some(dir) = env::var_os("GHOSTTY_VT_LIB_DIR") {
        let dir = PathBuf::from(dir);
        require_patched(Some(&dir), &format!("the library in {}", dir.display()))?;
        emit_link(&dir, link, &target);
        return Ok(());
    }

    let prefix = build_from_source(&target)?;
    // A cached build-script result names this library without checking it still exists. When a
    // CI cache restores `target/` but not the library cache, rerun instead of failing to link.
    println!(
        "cargo:rerun-if-changed={}",
        prefix.join("lib").join(static_lib_name(&target)).display()
    );
    println!("cargo:include={}", prefix.join("include").display());
    emit_link(&prefix.join("lib"), Link::Static, &target);
    Ok(())
}

fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

/// A library this script did not build lacks [`patch::PATCH`] unless the marker this script
/// installs next to its own libraries says otherwise.
fn require_patched(lib_dir: Option<&Path>, what: &str) -> BuildResult<()> {
    let expected = patch::marker()?;
    if let Some(dir) = lib_dir
        && fs::read_to_string(dir.join(patch::MARKER)).is_ok_and(|found| found == expected)
    {
        return Ok(());
    }
    if env_flag("GHOSTTY_VT_ALLOW_UNPATCHED") {
        println!(
            "cargo:warning=linking {what} without {}; GHOSTTY_VT_ALLOW_UNPATCHED is set",
            patch::PATCH
        );
        return Ok(());
    }
    Err(format!(
        "{what} may lack {patch}. It fixes upstream libghostty-vt bugs (alternate-screen \
         shrink, graphemes split across writes) and adds the `ghostty_terminal_tl_*` functions \
         tui-lipan's engine calls.\n\
         Either build from source (unset GHOSTTY_VT_SYSTEM and GHOSTTY_VT_LIB_DIR), point \
         GHOSTTY_VT_LIB_DIR at a lib/ directory this crate built (it holds {marker}), or set \
         GHOSTTY_VT_ALLOW_UNPATCHED=1 for a library that carries an equivalent patch; an \
         unpatched one fails to link.",
        patch = patch::PATCH,
        marker = patch::MARKER,
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum Link {
    Static,
    Dylib,
}

fn link_kind(msvc: bool) -> BuildResult<Link> {
    match env::var("GHOSTTY_VT_LINK").as_deref() {
        Ok("static") => Ok(Link::Static),
        Ok("dylib") => Ok(Link::Dylib),
        Ok(other) => Err(format!(
            "GHOSTTY_VT_LINK must be `static` or `dylib`, not `{other}`"
        )),
        Err(_) if msvc => Ok(Link::Dylib),
        Err(_) => Ok(Link::Static),
    }
}

fn link_pkg_config(link: Link) -> BuildResult<()> {
    let (name, statik) = match link {
        Link::Static => ("libghostty-vt-static", true),
        Link::Dylib => ("libghostty-vt", false),
    };
    let lib = pkg_config::Config::new()
        .atleast_version("1.0")
        .statik(statik)
        .probe(name)
        .map_err(|e| format!("pkg-config could not find {name}: {e}"))?;
    if let Some(include) = lib.include_paths.first() {
        println!("cargo:include={}", include.display());
    }
    Ok(())
}

fn emit_link(dir: &Path, link: Link, target: &str) {
    println!("cargo:rustc-link-search=native={}", dir.display());
    let windows = target.contains("-windows-");
    match link {
        // The Windows install tree names its static library `ghostty-vt-static.lib`, a name
        // the GNU toolchain only finds when given verbatim.
        Link::Static if windows && dir.join("ghostty-vt-static.lib").exists() => {
            println!("cargo:rustc-link-lib=static:+verbatim=ghostty-vt-static.lib");
        }
        Link::Static => println!("cargo:rustc-link-lib=static=ghostty-vt"),
        Link::Dylib => println!("cargo:rustc-link-lib=dylib=ghostty-vt"),
    }
    if windows && link == Link::Static {
        println!("cargo:rustc-link-lib=ntdll");
    }
}

/// The Zig settings that affect the installed library.
struct BuildSettings {
    optimize: String,
    cpu: String,
    zig_target: Option<String>,
}

/// Builds the static library into a cached install prefix and returns that prefix.
fn build_from_source(target: &str) -> BuildResult<PathBuf> {
    let cache = cache_dir()?;
    let host = env::var("HOST").map_err(|e| e.to_string())?;
    let settings = BuildSettings {
        optimize: env::var("GHOSTTY_VT_OPTIMIZE").unwrap_or_else(|_| "ReleaseFast".into()),
        cpu: env::var("GHOSTTY_VT_CPU").unwrap_or_else(|_| "baseline".into()),
        zig_target: zig_target(target, &host)?,
    };
    let key = source_build_key(&settings, target)?;
    let build_dir = cache.join("build").join(&key);
    let prefix = build_dir.join("prefix");
    if prefix.join("lib").join(static_lib_name(target)).exists() {
        return Ok(prefix);
    }

    let _lock = lock_build_directory(&build_dir)?;
    // Another build of the same configuration (a parallel `cargo test` and `cargo clippy`)
    // finishes first and this one reuses its result.
    if prefix.join("lib").join(static_lib_name(target)).exists() {
        return Ok(prefix);
    }

    compile_library(&build_dir, &settings)?;
    Ok(prefix)
}

/// Hold the per-key lock until compilation and installation finish.
fn lock_build_directory(build_dir: &Path) -> BuildResult<File> {
    fs::create_dir_all(build_dir).map_err(io_err("create", build_dir))?;
    let lock_path = build_dir.join(".lock");
    let lock = File::create(&lock_path).map_err(io_err("create", &lock_path))?;
    lock.lock().map_err(io_err("lock", &lock_path))?;
    Ok(lock)
}

/// Fingerprint the actual pinned package metadata as well as the recipe and Zig settings.
/// Hashing its manifest includes the resolved package version; hashing PATCHES includes every
/// applied patch digest. Cargo watches both, even if a local dependency override edits them.
fn source_build_key(settings: &BuildSettings, rust_target: &str) -> BuildResult<String> {
    let manifest = tui_lipan_ghostty_vt_src::root().join("Cargo.toml");
    let patches = tui_lipan_ghostty_vt_src::patches_file();
    let read = |path: &Path| {
        println!("cargo:rerun-if-changed={}", path.display());
        fs::read(path).map_err(io_err("read", path))
    };
    let manifest = read(&manifest)?;
    let patches = read(patches)?;
    let patch_digest = patch::marker()?;
    let digest = cache::build_key(
        cache::RECIPE,
        &[
            ("version", VERSION.as_bytes()),
            ("rust-target", rust_target.as_bytes()),
            (
                "target",
                settings
                    .zig_target
                    .as_deref()
                    .unwrap_or("native")
                    .as_bytes(),
            ),
            ("optimize", settings.optimize.as_bytes()),
            ("cpu", settings.cpu.as_bytes()),
            ("patch", patch_digest.as_bytes()),
            ("source-manifest", &manifest),
            ("source-patches", &patches),
        ],
    );
    Ok(format!("{VERSION}-{digest}"))
}

/// Compile and install while the caller holds this configuration's cache lock.
fn compile_library(build_dir: &Path, settings: &BuildSettings) -> BuildResult<()> {
    let (source, deps, patched) = source_trees()?;
    let zig = env::var("ZIG").unwrap_or_else(|_| "zig".into());
    check_zig_version(&zig)?;
    let source = patch::prepare(&source, build_dir, !patched)?;
    let staging = build_dir.join("staging");
    let _ = fs::remove_dir_all(&staging);

    let mut cmd = Command::new(&zig);
    cmd.current_dir(&source)
        .arg("build")
        .arg("--prefix")
        .arg(&staging)
        .arg("--system")
        .arg(&deps)
        .arg("--cache-dir")
        .arg(build_dir.join("zig-local"))
        .arg(format!("-Doptimize={}", settings.optimize))
        .arg(format!("-Dcpu={}", settings.cpu))
        .arg(format!("-Dlib-version-string={VERSION}"))
        // Rust links its own debug info; a stripped archive keeps it out of every binary.
        .arg("-Dstrip=true")
        // `--system` reads every package from `deps`. Zig 0.17 can crash on a global cache that
        // holds package archives, so use one that only this offline build ever touches.
        .env("ZIG_GLOBAL_CACHE_DIR", build_dir.join("zig-global"))
        // The cache usually sits inside the consumer's Git checkout. Ghostty's build reads
        // version tags with `git describe` and panics on tags that are not its own, so keep
        // Git from discovering any repository above the source tree.
        .env(
            "GIT_CEILING_DIRECTORIES",
            source.parent().unwrap_or(&source),
        );
    if let Some(zig_target) = &settings.zig_target {
        cmd.arg(format!("-Dtarget={zig_target}"));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("could not run `{zig}` ({e}).\n{}", zig_install_hint()))?;
    if !status.success() {
        return Err(format!(
            "`{zig} build` failed with {status} in {}",
            source.display()
        ));
    }

    install_prefix(build_dir, &staging, &source)
}

fn install_prefix(build_dir: &Path, staging: &Path, source: &Path) -> BuildResult<()> {
    let prefix = build_dir.join("prefix");
    let marker = staging.join("lib").join(patch::MARKER);
    fs::write(&marker, patch::marker()?).map_err(io_err("write", &marker))?;
    let _ = fs::remove_dir_all(&prefix);
    fs::rename(staging, &prefix).map_err(io_err("rename", staging))?;
    // Only a rebuild of this exact configuration could use Zig's caches or the private source
    // copy, and the installed prefix makes that unnecessary. Together they hold several hundred
    // MB, which would also bloat any CI cache of this directory.
    let _ = fs::remove_dir_all(build_dir.join("zig-local"));
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(build_dir.join("zig-global"));
    Ok(())
}

/// Fails with install instructions unless `zig` is the required release series. A different
/// series fails deep inside the build with unrelated compile errors (`@backingInt`, `std.lang`)
/// before libghostty-vt's own version check can run.
fn check_zig_version(zig: &str) -> BuildResult<()> {
    let output = Command::new(zig).arg("version").output().map_err(|e| {
        format!(
            "libghostty-vt {VERSION} needs Zig {}.x, and `{zig}` could not be run ({e}).\n{}",
            versions::ZIG_VERSION,
            zig_install_hint()
        )
    })?;
    let found = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let series = format!("{}.", versions::ZIG_VERSION);
    if output.status.success() && found.starts_with(&series) {
        return Ok(());
    }
    let found = if found.is_empty() {
        format!("unusable ({})", output.status)
    } else {
        found
    };
    Err(format!(
        "libghostty-vt {VERSION} needs Zig {}.x, but `{zig}` is {found}.\n{}",
        versions::ZIG_VERSION,
        zig_install_hint()
    ))
}

fn zig_install_hint() -> String {
    let zig = versions::ZIG_VERSION;
    format!(
        "  Install Zig {zig}: `mise use zig@{zig}`, or https://ziglang.org/download/\n  \
         Then put it first on PATH, or set ZIG=/path/to/zig.\n  \
         Not using the Ghostty engine? Turn off the feature that enables ghostty-vt-sys \
         (tui-lipan `terminal-ghostty`, Rozi `ghostty-engine`)."
    )
}

fn static_lib_name(target: &str) -> &'static str {
    if target.contains("-windows-") {
        "ghostty-vt-static.lib"
    } else {
        "libghostty-vt.a"
    }
}

/// The cache shared by every profile: `<target>/ghostty-vt`, or `GHOSTTY_VT_CACHE_DIR`.
fn cache_dir() -> BuildResult<PathBuf> {
    if let Some(dir) = env::var_os("GHOSTTY_VT_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    // OUT_DIR is `<target>/<profile>/build/<pkg>-<hash>/out`, or one level deeper when a
    // `--target` triple is given. The `build` component anchors the walk either way.
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);
    let target_dir = out
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "build"))
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| format!("cannot find the target directory above {}", out.display()))?;
    Ok(target_dir.join("ghostty-vt"))
}

/// Returns the source tree, the Zig package directory, and whether the source already carries
/// [`patch::PATCH`].
fn source_trees() -> BuildResult<(PathBuf, PathBuf, bool)> {
    if let Some(source) = env::var_os("GHOSTTY_VT_SOURCE_DIR") {
        let deps = env::var_os("GHOSTTY_VT_DEPS_DIR")
            .ok_or("GHOSTTY_VT_SOURCE_DIR also needs GHOSTTY_VT_DEPS_DIR")?;
        return Ok((PathBuf::from(source), PathBuf::from(deps), false));
    }

    let source = tui_lipan_ghostty_vt_src::source_dir();
    let deps = tui_lipan_ghostty_vt_src::deps_dir();
    let manifest = tui_lipan_ghostty_vt_src::patches_file();
    if !source.join("LIB_VERSION").is_file() || !deps.is_dir() {
        return Err(format!(
            "tui-lipan-ghostty-vt-src at {} holds no vendored source. In the tui-lipan \
             repository, generate it with scripts/package-ghostty-src.sh.",
            tui_lipan_ghostty_vt_src::root().display()
        ));
    }
    let lib_version = fs::read_to_string(source.join("LIB_VERSION"))
        .map_err(io_err("read", &source.join("LIB_VERSION")))?;
    if lib_version.trim() != VERSION {
        return Err(format!(
            "tui-lipan-ghostty-vt-src vendors libghostty-vt {}, but these bindings are for {VERSION}",
            lib_version.trim()
        ));
    }
    let applied = fs::read_to_string(manifest).map_err(io_err("read", manifest))?;
    if applied != patch::marker()? {
        return Err(format!(
            "tui-lipan-ghostty-vt-src carries other patches than this crate ({}):\n{applied}\
             Regenerate it with scripts/package-ghostty-src.sh and bump its version.",
            patch::PATCH
        ));
    }
    Ok((source.to_path_buf(), deps.to_path_buf(), true))
}

fn sha256_file(path: &Path) -> BuildResult<String> {
    let mut file = File::open(path).map_err(io_err("open", path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(io_err("read", path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The Zig `-Dtarget` for `target`, or `None` to let Zig build for the build machine.
///
/// A native build bakes in the host's glibc and macOS version, so the library may need newer
/// symbols than the Rust binary it ends up in (`arc4random_buf` needs glibc 2.36). Any cross
/// build, MSVC (whose objects Zig only produces when asked), or an explicit glibc or macOS floor
/// therefore names the target in full.
fn zig_target(target: &str, host: &str) -> BuildResult<Option<String>> {
    if let Some(explicit) = env::var("GHOSTTY_VT_ZIG_TARGET")
        .ok()
        .filter(|t| !t.is_empty())
    {
        return Ok(Some(explicit));
    }
    let mut zig = ZigTarget::from_rust(target)?;
    if zig.os == "linux" && zig.abi == Some("gnu") {
        zig.abi_version = version_env("GHOSTTY_VT_GLIBC")?;
    }
    if zig.os == "macos" {
        zig.os_version = version_env("MACOSX_DEPLOYMENT_TARGET")?;
    }
    let pinned = zig.os_version.is_some() || zig.abi_version.is_some();
    if target == host && zig.abi != Some("msvc") && !pinned {
        return Ok(None);
    }
    Ok(Some(zig.to_string()))
}

/// A dotted numeric version from `var`, if set.
fn version_env(var: &str) -> BuildResult<Option<String>> {
    match env::var(var) {
        Ok(v) if v.is_empty() => Ok(None),
        Ok(v)
            if v.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) =>
        {
            Ok(Some(v))
        }
        Ok(v) => Err(format!(
            "{var} must be a version such as 2.28 or 13.0, not `{v}`"
        )),
        Err(_) => Ok(None),
    }
}

/// Zig's `arch-os[.version][-abi[.version]]` triple.
struct ZigTarget {
    arch: &'static str,
    os: &'static str,
    os_version: Option<String>,
    abi: Option<&'static str>,
    abi_version: Option<String>,
}

impl ZigTarget {
    fn from_rust(target: &str) -> BuildResult<Self> {
        let mut parts = target.split('-');
        let arch = match parts.next().unwrap_or_default() {
            "x86_64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "i686" | "i586" => "x86",
            "riscv64gc" => "riscv64",
            other => {
                return Err(format!(
                    "no Zig target for architecture `{other}` of {target}"
                ));
            }
        };
        let rest: Vec<&str> = parts.collect();
        let has = |s: &str| rest.contains(&s);
        let (os, abi) = if has("linux") {
            ("linux", Some(if has("musl") { "musl" } else { "gnu" }))
        } else if has("darwin") {
            ("macos", None)
        } else if has("windows") {
            ("windows", Some(if has("msvc") { "msvc" } else { "gnu" }))
        } else if has("freebsd") {
            ("freebsd", None)
        } else {
            return Err(format!("no Zig target for {target}"));
        };
        Ok(Self {
            arch,
            os,
            os_version: None,
            abi,
            abi_version: None,
        })
    }
}

impl std::fmt::Display for ZigTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.arch, self.os)?;
        if let Some(v) = &self.os_version {
            write!(f, ".{v}")?;
        }
        if let Some(abi) = self.abi {
            write!(f, "-{abi}")?;
            if let Some(v) = &self.abi_version {
                write!(f, ".{v}")?;
            }
        }
        Ok(())
    }
}

fn io_err<'a>(what: &'a str, path: &'a Path) -> impl FnOnce(io::Error) -> String + 'a {
    move |e| format!("cannot {what} {}: {e}", path.display())
}
