#!/usr/bin/env python3
"""Bump the tui-lipan release version in every place that names it.

    python3 scripts/bump_version.py 0.12.2            # edit files only
    python3 scripts/bump_version.py 0.13.0 --commit   # also commit + tag
    python3 scripts/bump_version.py 0.12.2 --dry-run  # show what would change

Rewrites the `tui-lipan` and `tui-lipan-macro` package versions and the root
`tui-lipan-macro` dependency, then runs `check-install-snippets.py --fix` so a
series bump (0.12 -> 0.13) also updates the documented install snippets, and
finishes with `cargo check --workspace`.

With `--commit` it creates the signed-off `release: vX.Y.Z` commit and the
`vX.Y.Z` tag on `main`. Pushing is left to the maintainer; the command to run is
printed at the end.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ROOT_MANIFEST = ROOT / "Cargo.toml"
MACRO_MANIFEST = ROOT / "tui-lipan-macro" / "Cargo.toml"

# The release workflow only accepts stable `X.Y.Z` tags.
VERSION_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")

# The first `version = ` line of a manifest is the `[package]` version.
PACKAGE_VERSION_RE = re.compile(r'^(version\s*=\s*)"[^"]*"', re.MULTILINE)

MACRO_DEPENDENCY_RE = re.compile(
    r'^(tui-lipan-macro\s*=\s*\{[^}\n]*\bversion\s*=\s*)"[^"]*"', re.MULTILINE
)


class BumpError(Exception):
    pass


def parse_version(version: str) -> tuple[int, int, int]:
    match = VERSION_RE.match(version)
    if not match:
        raise BumpError(f"{version!r} is not a stable X.Y.Z version")
    major, minor, patch = (int(part) for part in match.groups())
    return major, minor, patch


def package_version(manifest: Path) -> str:
    with manifest.open("rb") as file:
        return tomllib.load(file)["package"]["version"]


def replace_once(pattern: re.Pattern[str], text: str, version: str, what: str) -> str:
    new_text, count = pattern.subn(rf'\g<1>"{version}"', text, count=1)
    if count != 1:
        raise BumpError(f"could not find the {what}")
    return new_text


def bump_root_manifest(text: str, version: str) -> str:
    text = replace_once(PACKAGE_VERSION_RE, text, version, "tui-lipan package version")
    return replace_once(
        MACRO_DEPENDENCY_RE, text, version, "tui-lipan-macro dependency version"
    )


def bump_macro_manifest(text: str, version: str) -> str:
    return replace_once(
        PACKAGE_VERSION_RE, text, version, "tui-lipan-macro package version"
    )


def run(*args: str, capture: bool = False) -> str:
    result = subprocess.run(
        args, cwd=ROOT, check=False, text=True, capture_output=capture
    )
    if result.returncode != 0:
        detail = f"\n{result.stderr.strip()}" if capture and result.stderr else ""
        raise BumpError(f"`{' '.join(args)}` failed{detail}")
    return result.stdout.strip() if capture else ""


def check_git_state(tag: str, commit: bool) -> None:
    if run("git", "status", "--porcelain", capture=True):
        raise BumpError("the working tree has uncommitted changes")
    tags = run("git", "tag", "--list", tag, capture=True)
    if tags:
        raise BumpError(f"tag {tag} already exists")
    if commit:
        branch = run("git", "branch", "--show-current", capture=True)
        if branch != "main":
            raise BumpError(f"--commit releases from main, not {branch or 'a detached HEAD'}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("version", help="the new version, e.g. 0.12.2")
    parser.add_argument(
        "--commit",
        action="store_true",
        help="commit as `release: vX.Y.Z` with a sign-off and create the tag",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="report the bump without writing"
    )
    args = parser.parse_args()

    try:
        new = parse_version(args.version)
        current_version = package_version(ROOT_MANIFEST)
        macro_version = package_version(MACRO_MANIFEST)
        if macro_version != current_version:
            raise BumpError(
                f"tui-lipan is {current_version} but tui-lipan-macro is "
                f"{macro_version}; fix the manifests by hand first"
            )
        if new <= parse_version(current_version):
            raise BumpError(
                f"{args.version} is not newer than the current {current_version}"
            )

        tag = f"v{args.version}"
        root_text = bump_root_manifest(ROOT_MANIFEST.read_text("utf-8"), args.version)
        macro_text = bump_macro_manifest(MACRO_MANIFEST.read_text("utf-8"), args.version)

        print(f"{current_version} -> {args.version}", flush=True)
        if args.dry_run:
            return 0

        check_git_state(tag, args.commit)
        ROOT_MANIFEST.write_text(root_text, "utf-8")
        MACRO_MANIFEST.write_text(macro_text, "utf-8")

        snippets = [sys.executable, "scripts/check-install-snippets.py"]
        run(*snippets, "--fix")
        run(*snippets)
        run("cargo", "check", "--quiet", "--workspace")

        if not args.commit:
            print(f"\nreview `git diff`, then commit `release: {tag}` and tag {tag}")
            return 0

        run("git", "commit", "--quiet", "--signoff", "--all", "-m", f"release: {tag}")
        run("git", "tag", tag)
        print(f"\ncommitted and tagged {tag}; publish with:\n")
        print(f"    git push origin main {tag}")
        return 0
    except BumpError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
