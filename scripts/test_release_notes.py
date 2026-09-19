import unittest

from scripts.release_notes import (
    ReleaseNotesError,
    exclusion_reason,
    select_previous_release,
    validate_notes,
)


class PreviousReleaseTests(unittest.TestCase):
    def test_selects_latest_published_v_tag_other_than_target(self):
        releases = [
            {
                "id": 4,
                "tag_name": "v0.11.7",
                "draft": False,
                "prerelease": True,
                "published_at": "2026-09-20T12:00:00Z",
            },
            {
                "id": 3,
                "tag_name": "nightly",
                "draft": False,
                "prerelease": True,
                "published_at": "2026-09-20T11:00:00Z",
            },
            {
                "id": 2,
                "tag_name": "v0.11.6",
                "draft": False,
                "prerelease": False,
                "published_at": "2026-09-19T12:00:00Z",
            },
            {
                "id": 1,
                "tag_name": "v0.11.5",
                "draft": True,
                "prerelease": False,
                "published_at": "2026-09-18T12:00:00Z",
            },
        ]

        self.assertEqual(select_previous_release(releases, "v0.11.7"), "v0.11.6")


class FilteringTests(unittest.TestCase):
    def test_filters_only_obvious_isolated_changes(self):
        self.assertEqual(
            exclusion_reason("ci: tune cache", [".github/workflows/ci.yml"]),
            "CI-only change",
        )
        self.assertEqual(
            exclusion_reason("test: add fixture", ["tests/fixtures/input.txt"]),
            "test-only change",
        )
        self.assertEqual(
            exclusion_reason("docs: explain focus", ["docs/focus.md"]),
            "documentation-only change",
        )
        self.assertEqual(
            exclusion_reason(
                "release: v0.11.7",
                ["Cargo.toml", "tui-lipan-macro/Cargo.toml"],
            ),
            "release metadata only",
        )

    def test_keeps_changes_that_may_be_user_visible(self):
        self.assertIsNone(exclusion_reason("perf: reduce redraws", ["src/app/runner.rs"]))
        self.assertIsNone(
            exclusion_reason(
                "chore(deps): update renderer",
                ["Cargo.toml", "Cargo.lock"],
            )
        )
        self.assertIsNone(
            exclusion_reason(
                "test: cover focus regression",
                ["src/app/input/focus.rs"],
            )
        )


class ValidationTests(unittest.TestCase):
    def test_accepts_nonempty_sections_in_fixed_order(self):
        notes = """\
## Added

- `List::navigation_wrap` now wraps selection at either end when enabled.

## Fixed

- Wrapped text now preserves cells rendered beneath a clipped region.
  Existing overlays keep their underlay.

## Compatibility

- `LegacyButton` has been removed. Use `Button` instead.
"""
        self.assertEqual(validate_notes(notes), notes)

    def test_rejects_unknown_or_reordered_sections(self):
        with self.assertRaises(ReleaseNotesError):
            validate_notes("## Fixed\n\n- One.\n\n## Added\n\n- Two.\n")
        with self.assertRaises(ReleaseNotesError):
            validate_notes("## Internal\n\n- Refactored modules.\n")

    def test_rejects_preamble_code_fences_and_empty_sections(self):
        with self.assertRaises(ReleaseNotesError):
            validate_notes("Release notes\n\n## Fixed\n\n- One.\n")
        with self.assertRaises(ReleaseNotesError):
            validate_notes("## Fixed\n\n```text\none\n```\n")
        with self.assertRaises(ReleaseNotesError):
            validate_notes("## Fixed\n")

    def test_rejects_ai_tell_punctuation(self):
        with self.assertRaises(ReleaseNotesError):
            validate_notes("## Fixed\n\n- Lists wrap — without skipping disabled rows.\n")
        with self.assertRaises(ReleaseNotesError):
            validate_notes('## Changed\n\n- The API calls this mode “capturing”.\n')


if __name__ == "__main__":
    unittest.main()
