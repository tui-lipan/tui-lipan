#!/usr/bin/env python3
"""Validate and optionally normalize the Keep a Changelog structure."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHANGELOG = ROOT / "CHANGELOG.md"
CATEGORIES = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")
RELEASE_RE = re.compile(r"^## \[([^]]+)](?: - (\d{4}-\d{2}-\d{2}))?$")
CATEGORY_RE = re.compile(r"^### (.+)$")
REFERENCE_RE = re.compile(r"^\[([^]]+)]: (\S+)$")
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")
REPOSITORY_URL = "https://github.com/tui-lipan/tui-lipan"


@dataclass
class Release:
    name: str
    date: str | None
    heading: str
    preamble: list[str] = field(default_factory=list)
    sections: list[tuple[str, list[str]]] = field(default_factory=list)


def parse_releases(lines: list[str]) -> tuple[list[str], list[Release], list[str]]:
    starts = [i for i, line in enumerate(lines) if RELEASE_RE.match(line)]
    if not starts:
        raise ValueError("no release headings found")

    prefix = lines[: starts[0]]
    releases: list[Release] = []
    release_names = {
        match.group(1)
        for start in starts
        if (match := RELEASE_RE.match(lines[start])) is not None
    }
    references_start = len(lines)
    while references_start > starts[-1] + 1:
        line = lines[references_start - 1]
        reference = REFERENCE_RE.match(line)
        if (reference and reference.group(1) in release_names) or not line:
            references_start -= 1
            continue
        break
    while references_start < len(lines) and not lines[references_start]:
        references_start += 1

    for position, start in enumerate(starts):
        end = starts[position + 1] if position + 1 < len(starts) else references_start
        match = RELEASE_RE.match(lines[start])
        assert match is not None
        release = Release(match.group(1), match.group(2), lines[start])
        current: tuple[str, list[str]] | None = None
        for line in lines[start + 1 : end]:
            category = CATEGORY_RE.match(line)
            if category:
                current = (category.group(1), [])
                release.sections.append(current)
            elif current is None:
                release.preamble.append(line)
            else:
                current[1].append(line)
        releases.append(release)

    return prefix, releases, lines[references_start:]


def normalize_blank_lines(lines: list[str]) -> list[str]:
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return lines


def entries(body: list[str]) -> list[list[str]]:
    found: list[list[str]] = []
    current: list[str] | None = None
    for line in body:
        if line.startswith("- "):
            current = [line]
            found.append(current)
        elif current is not None:
            current.append(line)
    return found


def render(prefix: list[str], releases: list[Release], references: list[str]) -> str:
    output = normalize_blank_lines(prefix.copy())
    for release in releases:
        output.extend(["", release.heading])
        preamble = normalize_blank_lines(release.preamble.copy())
        if preamble:
            output.extend(["", *preamble])

        grouped: dict[str, list[str]] = {category: [] for category in CATEGORIES}
        for category, body in release.sections:
            body = normalize_blank_lines(body.copy())
            if body:
                if grouped[category]:
                    grouped[category].append("")
                grouped[category].extend(body)
        for category in CATEGORIES:
            if grouped[category]:
                output.extend(["", f"### {category}", "", *grouped[category]])

    references = normalize_blank_lines(references.copy())
    if references:
        output.extend(["", *references])
    return "\n".join(output) + "\n"


def release_notes(releases: list[Release], version: str) -> str:
    """Render one release's body the way GitHub Release notes want it."""
    for release in releases:
        if release.name != version:
            continue
        output: list[str] = normalize_blank_lines(release.preamble.copy())
        for category, body in release.sections:
            body = normalize_blank_lines(body.copy())
            if not body:
                continue
            if output:
                output.append("")
            output.extend([f"### {category}", "", *body])
        return "\n".join(output) + "\n"
    raise ValueError(f"no release section for {version}")


def validate(releases: list[Release], references: list[str]) -> list[str]:
    problems: list[str] = []
    names = [release.name for release in releases]
    release_names = set(names)
    reference_pairs = [
        (match.group(1), match.group(2))
        for line in references
        if (match := REFERENCE_RE.match(line)) is not None
    ]
    parsed_references = dict(reference_pairs)
    reference_names = set(parsed_references)

    if names.count("Unreleased") != 1 or releases[0].name != "Unreleased":
        problems.append("the first release must be [Unreleased]")
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        problems.append(f"duplicate releases: {', '.join(duplicates)}")
    duplicate_references = sorted(
        {name for name, _ in reference_pairs if [pair[0] for pair in reference_pairs].count(name) > 1}
    )
    if duplicate_references:
        problems.append(f"duplicate link references: {', '.join(duplicate_references)}")

    for release in releases:
        if release.name == "Unreleased" and release.date is not None:
            problems.append("[Unreleased] must not have a date")
        if release.name != "Unreleased" and release.date is None:
            problems.append(f"[{release.name}] must have an ISO release date")
        if release.name != "Unreleased" and not VERSION_RE.fullmatch(release.name):
            problems.append(f"[{release.name}] is not a semantic X.Y.Z version")
        if release.date is not None:
            try:
                date.fromisoformat(release.date)
            except ValueError:
                problems.append(f"[{release.name}] has invalid date {release.date}")
        if release.name == "Unreleased" and any(release.preamble):
            problems.append("[Unreleased] content must be under a category heading")

        categories = [category for category, _ in release.sections]
        unknown = [category for category in categories if category not in CATEGORIES]
        duplicates = sorted({category for category in categories if categories.count(category) > 1})
        if unknown:
            problems.append(f"[{release.name}] has unknown categories: {', '.join(unknown)}")
        if duplicates:
            problems.append(f"[{release.name}] repeats categories: {', '.join(duplicates)}")
        known = [category for category in categories if category in CATEGORIES]
        if known != sorted(known, key=CATEGORIES.index):
            problems.append(f"[{release.name}] categories are out of order")

        if release.name == "Unreleased":
            for category, body in release.sections:
                for entry in entries(body):
                    text = "\n".join(entry)
                    if "(breaking)" not in text.lower():
                        continue
                    final_line = next(line for line in reversed(entry) if line)
                    if not final_line.rstrip().endswith("(breaking)"):
                        problems.append(
                            f"[Unreleased] {category} entry starting {entry[0]!r} "
                            "must end with (breaking)"
                        )

    missing_references = sorted(release_names - reference_names)
    extra_references = sorted(reference_names - release_names)
    if missing_references:
        problems.append(f"missing link references: {', '.join(missing_references)}")
    if extra_references:
        problems.append(f"link references without releases: {', '.join(extra_references)}")

    versions = [release.name for release in releases if release.name != "Unreleased"]
    if versions:
        expected = {
            "Unreleased": f"{REPOSITORY_URL}/compare/v{versions[0]}...HEAD",
            versions[-1]: f"{REPOSITORY_URL}/releases/tag/v{versions[-1]}",
        }
        for newer, older in zip(versions, versions[1:]):
            expected[newer] = f"{REPOSITORY_URL}/compare/v{older}...v{newer}"
        for name, url in expected.items():
            if name in parsed_references and parsed_references[name] != url:
                problems.append(
                    f"[{name}] link must be {url}, got {parsed_references[name]}"
                )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fix",
        action="store_true",
        help="merge duplicate category blocks and put categories in canonical order",
    )
    parser.add_argument(
        "--release-notes",
        metavar="VERSION",
        help="print that version's sections to stdout instead of validating",
    )
    args = parser.parse_args()

    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    try:
        prefix, releases, references = parse_releases(lines)
    except ValueError as error:
        print(f"CHANGELOG.md: {error}", file=sys.stderr)
        return 1

    if args.release_notes:
        try:
            sys.stdout.write(release_notes(releases, args.release_notes))
        except ValueError as error:
            print(f"CHANGELOG.md: {error}", file=sys.stderr)
            return 1
        return 0

    if args.fix:
        unknown = sorted(
            {
                category
                for release in releases
                for category, _ in release.sections
                if category not in CATEGORIES
            }
        )
        if unknown:
            print(
                f"CHANGELOG.md: refusing to move unknown categories: {', '.join(unknown)}",
                file=sys.stderr,
            )
            return 1
        candidate = render(prefix, releases, references)
        _, fixed_releases, fixed_references = parse_releases(candidate.splitlines())
        problems = validate(fixed_releases, fixed_references)
        if problems:
            for problem in problems:
                print(f"CHANGELOG.md: {problem}", file=sys.stderr)
            return 1
        CHANGELOG.write_text(candidate, encoding="utf-8")
        releases, references = fixed_releases, fixed_references

    problems = validate(releases, references)
    if problems:
        for problem in problems:
            print(f"CHANGELOG.md: {problem}", file=sys.stderr)
        print("Run `python3 scripts/check-changelog.py --fix` to regroup sections.", file=sys.stderr)
        return 1

    print(f"changelog has {len(releases)} releases with canonical section structure")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
