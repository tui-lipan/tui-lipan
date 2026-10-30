import unittest

from scripts.release_notes import (
    SUMMARY_MAX_CHARS,
    ReleaseNotesError,
    check_pull_request,
    commit_summary,
    exclusion_reason,
    is_breaking,
    pull_request_url,
    requires_note,
    select_previous_release,
    validate_coverage,
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


class SummaryTests(unittest.TestCase):
    def test_keeps_the_summary_section_and_drops_checklists(self):
        body = """\
## Summary

Exit views no longer query cursor position after teardown.

## Checklist

- [x] `cargo test` passes

Signed-off-by: Ada <ada@example.com>
"""
        self.assertEqual(
            commit_summary(body),
            "Exit views no longer query cursor position after teardown.",
        )

    def test_uses_the_preamble_when_there_is_no_summary_heading(self):
        body = """\
Live System theme updates refresh all 16 ANSI slots.

## Notes for reviewers

This uses one persistent parser.

---------
Co-authored-by: Ada <ada@example.com>
"""
        self.assertEqual(
            commit_summary(body),
            "Live System theme updates refresh all 16 ANSI slots.",
        )

    def test_strips_html_comments(self):
        body = """\
<!-- PR titles land in main's history -->

## Summary

`List::empty_text_padding` insets empty-state text.
"""
        self.assertEqual(
            commit_summary(body),
            "`List::empty_text_padding` insets empty-state text.",
        )


    def test_keeps_subsections_nested_under_the_summary(self):
        body = """\
## Summary

Text actions are bindable.

### Breaking changes

`FrameworkAction` is non-exhaustive. Add a wildcard arm.

## Checklist

- [x] tests pass
"""
        summary = commit_summary(body)
        self.assertIn("Text actions are bindable.", summary)
        self.assertIn("Add a wildcard arm.", summary)
        self.assertNotIn("tests pass", summary)

    def test_uses_every_release_section_when_there_is_no_summary_or_preamble(self):
        body = """\
## Problem

Keys took the type color.

## Changes

Keys now use the variable color.

## Testing

Replayed against fixtures.
"""
        summary = commit_summary(body)
        self.assertIn("Keys took the type color.", summary)
        self.assertIn("Keys now use the variable color.", summary)
        self.assertNotIn("fixtures", summary)

    def test_keeps_upgrade_guidance_outside_the_summary(self):
        body = """\
## Summary

Palettes gain slots.

## Migration

Set `tag` on struct literals.
"""
        self.assertIn("Set `tag` on struct literals.", commit_summary(body))

    def test_ignores_heading_like_lines_in_fenced_code(self):
        body = """\
## Summary

Run this:

```bash
# not a heading
cargo test
```

The tail of the summary.
"""
        summary = commit_summary(body)
        self.assertIn("cargo test", summary)
        self.assertIn("The tail of the summary.", summary)

    def test_truncation_keeps_breaking_changes(self):
        body = (
            "## Summary\n\n"
            + "\n".join("filler line" for _ in range(SUMMARY_MAX_CHARS))
            + "\n\n### Breaking changes\n\nAdd a wildcard arm.\n"
        )
        summary = commit_summary(body)
        self.assertLessEqual(len(summary), SUMMARY_MAX_CHARS + 2)
        self.assertIn("Add a wildcard arm.", summary)


class BreakingTests(unittest.TestCase):
    def test_reads_the_title_marker_and_the_section(self):
        self.assertTrue(is_breaking("fix(syntect)!: recolor (#1)", "## Summary\n\nText."))
        self.assertTrue(
            is_breaking("feat: add", "## Summary\n\nText.\n\n### Breaking changes\n\nMigrate.")
        )

    def test_ignores_prose_and_empty_or_negative_sections(self):
        self.assertFalse(is_breaking("fix: x", "There are no breaking API changes."))
        self.assertFalse(is_breaking("fix: x", "### Breaking changes\n\nNone."))
        self.assertFalse(is_breaking("fix: x", "### Breaking changes\n"))


def range_commit(subject, paths=("src/lib.rs",), included=True, breaking=False):
    return {
        "sha": subject,
        "subject": subject,
        "paths": list(paths),
        "included": included,
        "breaking": breaking,
    }


def notes_with(*sections):
    return "\n\n".join(f"## {name}\n- Entry. {link}" for name, link in sections) + "\n"


class CoverageTests(unittest.TestCase):
    LINK = "([#7](https://github.com/o/r/pull/7))"

    def test_requires_library_changes_of_user_facing_types(self):
        self.assertTrue(requires_note(range_commit("fix: x (#7)")))
        self.assertTrue(requires_note(range_commit("perf(a): x (#7)")))
        self.assertTrue(requires_note(range_commit("ci: x (#7)", breaking=True)))
        self.assertFalse(requires_note(range_commit("ci: x (#7)")))
        self.assertFalse(requires_note(range_commit("fix: x (#7)", paths=("tests/a.rs",))))
        self.assertFalse(requires_note(range_commit("fix: x (#7)", included=False)))

    def test_accepts_notes_covering_the_range(self):
        validate_coverage(notes_with(("Fixed", self.LINK)), [range_commit("fix: x (#7)")])

    def test_rejects_a_dropped_change(self):
        with self.assertRaisesRegex(ReleaseNotesError, "#7 is missing"):
            validate_coverage(notes_with(("Fixed", "Other.")), [range_commit("fix: x (#7)")])

    def test_rejects_a_breaking_change_outside_compatibility(self):
        commits = [range_commit("feat: x (#7)", breaking=True)]
        with self.assertRaisesRegex(ReleaseNotesError, "not in Compatibility"):
            validate_coverage(notes_with(("Fixed", self.LINK)), commits)
        validate_coverage(notes_with(("Compatibility", self.LINK)), commits)

    def test_rejects_a_pull_request_outside_the_range(self):
        notes = notes_with(("Fixed", self.LINK), ("Security", "(/pull/9)"))
        with self.assertRaisesRegex(ReleaseNotesError, "#9 is not in this release range"):
            validate_coverage(notes, [range_commit("fix: x (#7)")])

    def test_does_not_require_commits_without_a_pull_request(self):
        validate_coverage(notes_with(("Fixed", "Other.")), [range_commit("fix: pushed directly")])


class PullRequestCheckTests(unittest.TestCase):
    def test_accepts_a_summary(self):
        self.assertEqual(check_pull_request("fix(a): b", "## Summary\n\nWhat changed."), [])

    def test_accepts_a_summary_held_in_a_subsection(self):
        body = "## Summary\n\n### Fixed\n\nWhat changed."
        self.assertEqual(check_pull_request("fix: b", body), [])

    def test_rejects_a_missing_or_template_only_summary(self):
        self.assertIn("add a `## Summary`", check_pull_request("fix: b", "## Problem\n\nX.")[0])
        template = "## Summary\n\n<!-- What does this change? -->\n\n## Checklist\n\n- [ ] x\n"
        self.assertIn("is empty", check_pull_request("feat: b", template)[0])

    def test_requires_breaking_section_for_a_bang_title(self):
        problems = check_pull_request("fix!: b", "## Summary\n\nWhat changed.")
        self.assertIn("Breaking changes", problems[0])
        body = "## Summary\n\nX.\n\n### Breaking changes\n\nMigrate by doing Y."
        self.assertEqual(check_pull_request("fix!: b", body), [])

    def test_leaves_non_user_facing_types_alone(self):
        self.assertEqual(check_pull_request("ci: b", ""), [])
        self.assertEqual(check_pull_request("docs(a): b", ""), [])

    def test_rejects_a_title_that_is_not_conventional(self):
        self.assertIn("Conventional Commit", check_pull_request("Fix the thing", "")[0])
        self.assertIn("Conventional Commit", check_pull_request("wip: thing", "")[0])


class PullRequestUrlTests(unittest.TestCase):
    def test_reads_the_squash_merge_trailer(self):
        self.assertEqual(
            pull_request_url(
                "tui-lipan/tui-lipan",
                "fix(runner): render exit views without cursor queries (#250)",
            ),
            "https://github.com/tui-lipan/tui-lipan/pull/250",
        )

    def test_ignores_subjects_without_a_trailer(self):
        self.assertIsNone(
            pull_request_url("tui-lipan/tui-lipan", "release: v0.11.7")
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

    def test_accepts_pull_request_links(self):
        notes = """\
## Fixed

- Exit views no longer query cursor position after teardown.
  ([#250](https://github.com/tui-lipan/tui-lipan/pull/250))
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
