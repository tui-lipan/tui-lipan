import unittest

from scripts.bump_version import (
    ROOT_MANIFEST,
    MACRO_MANIFEST,
    BumpError,
    bump_macro_manifest,
    bump_root_manifest,
    parse_version,
)


ROOT_TEXT = """\
[package]
name = "tui-lipan"
version = "0.12.1"
edition = "2024"

[dependencies]
ratatui = { version = "0.29.0", default-features = false }
tui-lipan-macro = { path = "tui-lipan-macro", version = "0.12.1" }

[dev-dependencies]
insta = { version = "1.0" }
"""


class ParseVersionTests(unittest.TestCase):
    def test_accepts_stable_versions(self):
        self.assertEqual(parse_version("0.12.2"), (0, 12, 2))
        self.assertEqual(parse_version("1.0.0"), (1, 0, 0))

    def test_orders_numerically(self):
        self.assertLess(parse_version("0.9.9"), parse_version("0.10.0"))

    def test_rejects_non_release_versions(self):
        for version in ["0.12", "v0.12.2", "0.12.2-rc.1", "0.012.2", "0.12.2 "]:
            with self.subTest(version=version), self.assertRaises(BumpError):
                parse_version(version)


class ManifestRewriteTests(unittest.TestCase):
    def test_root_manifest_bumps_package_and_macro_dependency_only(self):
        bumped = bump_root_manifest(ROOT_TEXT, "0.13.0")
        self.assertEqual(
            bumped,
            ROOT_TEXT.replace('version = "0.12.1"\n', 'version = "0.13.0"\n', 1).replace(
                'version = "0.12.1" }', 'version = "0.13.0" }'
            ),
        )
        self.assertIn('ratatui = { version = "0.29.0"', bumped)

    def test_root_manifest_without_macro_dependency_is_an_error(self):
        text = ROOT_TEXT.replace("tui-lipan-macro = ", "other = ")
        with self.assertRaises(BumpError):
            bump_root_manifest(text, "0.13.0")

    def test_macro_manifest_bumps_package_version(self):
        text = '[package]\nname = "tui-lipan-macro"\nversion = "0.12.1"\n'
        self.assertEqual(
            bump_macro_manifest(text, "0.12.2"),
            '[package]\nname = "tui-lipan-macro"\nversion = "0.12.2"\n',
        )

    def test_real_manifests_still_match(self):
        # Guards the regexes against layout changes in the checked-in manifests.
        root = ROOT_MANIFEST.read_text("utf-8")
        bumped = bump_root_manifest(root, "99.0.0")
        self.assertEqual(bumped.count('"99.0.0"'), 2)
        macro = bump_macro_manifest(MACRO_MANIFEST.read_text("utf-8"), "99.0.0")
        self.assertEqual(macro.count('"99.0.0"'), 1)


if __name__ == "__main__":
    unittest.main()
