#!/usr/bin/env python3
"""Focused regression tests for check-changelog.py."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check-changelog.py")
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("check_changelog", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
check_changelog = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = check_changelog
SPEC.loader.exec_module(check_changelog)


class CheckChangelogTests(unittest.TestCase):
    def test_embedded_reference_stays_in_release(self) -> None:
        lines = """# Changelog

## [Unreleased]

### Added

- Read the [guide].

[guide]: docs/guide.md

## [0.1.0] - 2026-01-01

- First release.

[Unreleased]: https://github.com/tui-lipan/tui-lipan/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tui-lipan/tui-lipan/releases/tag/v0.1.0
""".splitlines()
        _, releases, references = check_changelog.parse_releases(lines)
        self.assertIn("[guide]: docs/guide.md", releases[0].sections[0][1])
        self.assertEqual(len(references), 2)

    def test_duplicate_release_is_rejected(self) -> None:
        release = check_changelog.Release("Unreleased", None, "## [Unreleased]")
        problems = check_changelog.validate([release, release], [])
        self.assertTrue(any("duplicate releases" in problem for problem in problems))

    def test_invalid_calendar_date_is_rejected(self) -> None:
        releases = [
            check_changelog.Release("Unreleased", None, "## [Unreleased]"),
            check_changelog.Release("0.1.0", "2026-02-30", "## [0.1.0] - 2026-02-30"),
        ]
        problems = check_changelog.validate(releases, [])
        self.assertTrue(any("invalid date" in problem for problem in problems))

    def test_unreleased_breaking_marker_must_end_entry(self) -> None:
        release = check_changelog.Release(
            "Unreleased",
            None,
            "## [Unreleased]",
            sections=[("Changed", ["- API changed (breaking).", "  Migration text."])],
        )
        problems = check_changelog.validate([release], [])
        self.assertTrue(any("must end with (breaking)" in problem for problem in problems))

    def test_render_merges_sections_and_is_idempotent(self) -> None:
        prefix = ["# Changelog", ""]
        releases = [
            check_changelog.Release(
                "Unreleased",
                None,
                "## [Unreleased]",
                sections=[
                    ("Fixed", ["", "- Fixed first.", ""]),
                    ("Added", ["", "- Added once.", ""]),
                    ("Fixed", ["", "- Fixed second.", ""]),
                ],
            ),
            check_changelog.Release(
                "0.1.0",
                "2026-01-01",
                "## [0.1.0] - 2026-01-01",
                preamble=["", "Initial release.", ""],
            ),
        ]
        references = [
            "[Unreleased]: https://github.com/tui-lipan/tui-lipan/compare/v0.1.0...HEAD",
            "[0.1.0]: https://github.com/tui-lipan/tui-lipan/releases/tag/v0.1.0",
        ]
        rendered = check_changelog.render(prefix, releases, references)
        parsed = check_changelog.parse_releases(rendered.splitlines())
        self.assertEqual(rendered, check_changelog.render(*parsed))
        self.assertEqual(rendered.count("### Fixed"), 1)
        self.assertLess(rendered.index("### Added"), rendered.index("### Fixed"))
        self.assertIn("- Fixed first.\n\n- Fixed second.", rendered)

    def test_unreleased_preamble_is_rejected(self) -> None:
        release = check_changelog.Release(
            "Unreleased",
            None,
            "## [Unreleased]",
            preamble=["", "- Uncategorized change.", ""],
        )
        problems = check_changelog.validate([release], [])
        self.assertTrue(any("category heading" in problem for problem in problems))


if __name__ == "__main__":
    unittest.main()
