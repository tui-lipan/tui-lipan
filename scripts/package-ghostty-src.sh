#!/usr/bin/env bash
# Generates the vendored crate tree in ghostty-vt-src/ (tui-lipan-ghostty-vt-src) from the pinned
# libghostty-vt release, with ghostty-vt-sys/patches/*.patch applied, then smoke-builds it offline
# with Zig and checks that the packaged crate stays under the size cap.
#
# Usage:
#   scripts/package-ghostty-src.sh                 generate, smoke-build, package, check size
#   scripts/package-ghostty-src.sh --no-smoke      skip the Zig smoke build
#   scripts/package-ghostty-src.sh --check-crate <file.crate | crates.io version>
#                                                  regenerate and compare with a packaged crate
#
# Environment:
#   ZIG                Zig 0.17.x (default `zig`)
#   MINISIGN           minisign binary (default `minisign` on PATH). Without it, signatures are
#                      not checked and the run says so; REQUIRE_MINISIGN=1 makes that an error.
#
# The tree is the crate's content and is gitignored: it is generated for publishing and for local
# work behind the workspace's [patch.crates-io] entry, never committed.
set -euo pipefail

VERSION=1.0.1
ZIG_SERIES=0.17
BASE_URL="https://release.files.ghostty.org/libghostty-vt/$VERSION"
# Ghostty's release key (PACKAGING-LIBGHOSTTY.md).
MINISIGN_KEY="RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV"
# From the release's SHA256SUMS.
SOURCE_TARBALL="libghostty-vt-$VERSION.tar.gz"
SOURCE_SHA256=333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa
DEPS_TARBALL="libghostty-vt-$VERSION-deps.tar.gz"
DEPS_SHA256=91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d
# crates.io rejects uploads over 10 MiB; keep 1 MiB of headroom for upstream growth.
MAX_CRATE_BYTES=$((9 * 1024 * 1024))
CRATE_NAME=tui-lipan-ghostty-vt-src

root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
crate="$root/ghostty-vt-src"
dl="$root/target/ghostty-vt/dl"
work="$root/target/ghostty-vt-src-work"
smoke=1
check_crate=""

while (($#)); do
  case "$1" in
    --no-smoke) smoke=0 ;;
    --check-crate)
      check_crate="${2:?--check-crate needs a .crate file or a crates.io version}"
      shift
      ;;
    -h | --help)
      sed -n '2,19p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

step() { printf '==> %s\n' "$*"; }

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }

grep -q "LIBGHOSTTY_VT_VERSION: &str = \"$VERSION\"" "$crate/src/lib.rs" || {
  echo "ghostty-vt-src/src/lib.rs does not name libghostty-vt $VERSION" >&2
  exit 1
}
grep -q "LIBGHOSTTY_VT_VERSION: &str = \"$VERSION\"" "$root/ghostty-vt-sys/src/versions.rs" || {
  echo "ghostty-vt-sys/src/versions.rs does not name libghostty-vt $VERSION" >&2
  exit 1
}

fetch() {
  local file="$1" sum="$2"
  mkdir -p "$dl"
  if [[ ! -f "$dl/$file" || "$(sha256_of "$dl/$file")" != "$sum" ]]; then
    curl --fail --location --silent --show-error --retry 3 -o "$dl/$file.part" "$BASE_URL/$file"
    mv "$dl/$file.part" "$dl/$file"
  fi
  local actual
  actual="$(sha256_of "$dl/$file")"
  if [[ "$actual" != "$sum" ]]; then
    echo "$file has SHA-256 $actual, expected $sum" >&2
    exit 1
  fi
}

verify_signature() {
  local file="$1" minisign="${MINISIGN:-minisign}"
  if ! command -v "$minisign" >/dev/null 2>&1; then
    if [[ "${REQUIRE_MINISIGN:-0}" == 1 ]]; then
      echo "minisign not found and REQUIRE_MINISIGN=1" >&2
      exit 1
    fi
    echo "    WARNING: minisign not found; $file is checked against its pinned SHA-256 only"
    return
  fi
  curl --fail --location --silent --show-error --retry 3 \
    -o "$dl/$file.minisig" "$BASE_URL/$file.minisig"
  "$minisign" -Vq -P "$MINISIGN_KEY" -m "$dl/$file" -x "$dl/$file.minisig"
  echo "    minisign signature verified: $file"
}

step "Fetching libghostty-vt $VERSION release tarballs"
fetch "$SOURCE_TARBALL" "$SOURCE_SHA256"
fetch "$DEPS_TARBALL" "$DEPS_SHA256"
verify_signature "$SOURCE_TARBALL"
verify_signature "$DEPS_TARBALL"

step "Unpacking and patching"
rm -rf "$work"
mkdir -p "$work"
tar -xzf "$dl/$SOURCE_TARBALL" -C "$work"
tar -xzf "$dl/$DEPS_TARBALL" -C "$work"
src="$work/libghostty-vt-$VERSION"
deps="$work/libghostty-vt-$VERSION-deps"
[[ "$(tr -d '[:space:]' <"$src/LIB_VERSION")" == "$VERSION" ]] || {
  echo "$SOURCE_TARBALL holds LIB_VERSION $(cat "$src/LIB_VERSION")" >&2
  exit 1
}

patches_manifest="$work/PATCHES"
: >"$patches_manifest"
shopt -s nullglob
for patch in "$root"/ghostty-vt-sys/patches/*.patch; do
  rel="patches/$(basename "$patch")"
  patch --directory="$src" --batch --forward --fuzz=0 -p1 --quiet -i "$patch"
  printf '%s %s\n' "$rel" "$(sha256_of "$patch")" >>"$patches_manifest"
  echo "    applied $rel"
done
shopt -u nullglob

step "Trimming tests, documentation and examples"
# translate_c's build.zig references its test/cases.zig, so that one test directory stays. Leave
# every other file alone: Zig hashes the paths a package's build.zig.zon lists, .gitignore
# included, and fails on any that is missing.
find "$src" "$deps" -depth -type d \
  \( -name docs -o -name g3doc -o -name examples -o -name testdata -o -name tests -o -name test \) \
  ! -path '*/translate_c-*/test' -exec rm -rf {} +
rm -rf "$src/nix" "$src/valgrind.supp"

step "Writing third-party notices"
python3 -I - "$src" "$deps" "$work/THIRD_PARTY.md" <<'PY'
import json, pathlib, sys

src, deps, out = (pathlib.Path(p) for p in sys.argv[1:])
entries = json.loads((src / "deps/libghostty-vt-third-party.json").read_text())["third_party"]

def licence_files(directory: pathlib.Path) -> list[str]:
    names = ("LICENSE", "LICENCE", "COPYING", "NOTICE")
    found = [
        p for p in directory.rglob("*")
        if p.is_file() and p.name.upper().startswith(names)
    ]
    return sorted(str(p.relative_to(out.parent)) for p in found)

lines = [
    "# Third-party code in tui-lipan-ghostty-vt-src",
    "",
    "Generated from `libghostty-vt/deps/libghostty-vt-third-party.json`. `linked` code is compiled",
    "into libghostty-vt; `build` code only runs during its build. Paths are relative to this crate;",
    "`libghostty-vt-deps` directories are named by Zig package hash.",
    "",
    "| Project | Version | Licence | Use | Licence files |",
    "| --- | --- | --- | --- | --- |",
    "| libghostty-vt | {} | MIT | linked | `libghostty-vt/LICENSE` |".format(
        (src / "LIB_VERSION").read_text().strip()
    ),
]
for e in entries:
    if e.get("kind") == "zig-package":
        files = licence_files(deps / e["hash"])
    else:
        files = licence_files(src / e.get("path", "pkg/" + e["name"]))
    if not files:
        sys.exit(f"no licence file found for {e['name']}")
    shown = "<br>".join(f"`{f}`" for f in files)
    lines.append(f"| {e['name']} | {e['version']} | {e['license']} | {e['use']} | {shown} |")
out.write_text("\n".join(lines) + "\n")
PY

step "Assembling ghostty-vt-src/"
assemble() {
  local into="$1"
  rm -rf "$into/libghostty-vt" "$into/libghostty-vt-deps" "$into/PATCHES" "$into/THIRD_PARTY.md"
  cp -a "$src" "$into/libghostty-vt"
  cp -a "$deps" "$into/libghostty-vt-deps"
  cp "$patches_manifest" "$into/PATCHES"
  # THIRD_PARTY.md was written with the work directory's names; the crate's are stable.
  sed -e "s#libghostty-vt-$VERSION-deps/#libghostty-vt-deps/#g" \
    -e "s#libghostty-vt-$VERSION/#libghostty-vt/#g" \
    "$work/THIRD_PARTY.md" >"$into/THIRD_PARTY.md"
}
assemble "$crate"
du -sh "$crate/libghostty-vt" "$crate/libghostty-vt-deps" | sed 's/^/    /'

if ((smoke)); then
  step "Smoke-building offline with Zig"
  zig="${ZIG:-zig}"
  found="$("$zig" version)"
  [[ "$found" == "$ZIG_SERIES".* ]] || {
    echo "the smoke build needs Zig $ZIG_SERIES.x, found $found ($zig)" >&2
    exit 1
  }
  # Build a copy, so nothing the build writes can end up in the published tree.
  cp -a "$crate/libghostty-vt" "$work/smoke-src"
  (
    cd "$work/smoke-src"
    ZIG_GLOBAL_CACHE_DIR="$work/zig-global" GIT_CEILING_DIRECTORIES="$work" \
      "$zig" build --prefix "$work/smoke-prefix" --system "$crate/libghostty-vt-deps" \
      --cache-dir "$work/zig-local" -Doptimize=ReleaseFast -Dcpu=baseline -Dstrip=true \
      "-Dlib-version-string=$VERSION"
  )
  test -f "$work/smoke-prefix/lib/libghostty-vt.a"
  echo "    built $(du -h "$work/smoke-prefix/lib/libghostty-vt.a" | cut -f1) libghostty-vt.a"
fi

step "Packaging"
export CARGO_TARGET_DIR="$root/target/ghostty-vt-src-package"
# --allow-dirty: the vendored trees are gitignored by design; the include list packs them.
cargo package --manifest-path "$crate/Cargo.toml" --no-verify --allow-dirty --quiet
crate_file="$(ls -t "$CARGO_TARGET_DIR"/package/$CRATE_NAME-*.crate | head -n 1)"
size="$(stat -c %s "$crate_file")"
echo "    $crate_file: $size bytes (cap $MAX_CRATE_BYTES)"
((size <= MAX_CRATE_BYTES)) || {
  echo "the packaged crate exceeds the cap; trim more or split the crate" >&2
  exit 1
}

if [[ -n "$check_crate" ]]; then
  step "Comparing with $check_crate"
  if [[ ! -f "$check_crate" ]]; then
    url="https://static.crates.io/crates/$CRATE_NAME/$CRATE_NAME-$check_crate.crate"
    curl --fail --location --silent --show-error -o "$work/published.crate" "$url"
    check_crate="$work/published.crate"
  fi
  mkdir -p "$work/published"
  tar -xzf "$check_crate" -C "$work/published"
  published="$(find "$work/published" -mindepth 1 -maxdepth 1 -type d)"
  # cargo normalizes Cargo.toml and keeps the original as Cargo.toml.orig.
  diff -q "$crate/Cargo.toml" "$published/Cargo.toml.orig"
  diff -r -q --exclude=Cargo.toml --exclude=Cargo.toml.orig --exclude=.cargo_vcs_info.json \
    --exclude=target --exclude=.gitignore --exclude=Cargo.lock "$crate" "$published"
  echo "    identical"
fi

rm -rf "$work"
step "Done: ghostty-vt-src/ is ready; publish with cargo publish --manifest-path ghostty-vt-src/Cargo.toml"
